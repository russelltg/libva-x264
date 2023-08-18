#![feature(array_try_from_fn)]

mod sys;

use dcp::{convert_image, ImageFormat, PixelFormat};
use dcv_color_primitives as dcp;

use std::{
    array,
    ffi::CStr,
    fmt,
    fs::File,
    mem::{self, size_of, MaybeUninit},
    num::{NonZeroIsize, NonZeroUsize},
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd},
        raw::{c_int, c_uint, c_void},
    },
    ptr::{null, null_mut, NonNull},
    slice,
};

use c_string::c_str;
// use gbm::{BufferObject, BufferObjectFlags, Device};
// use gles30::{GlFns, GL_RGB, GL_TEXTURE_2D, GL_UNSIGNED_BYTE};
// use khronos_egl::{
//     Boolean, Display, Dynamic, DynamicInstance, EGLDisplay, EGLImage, Instance, Int,
//     NativeDisplayType, Upcast, ALPHA_SIZE, ATTRIB_NONE, BLUE_SIZE, COLOR_BUFFER_TYPE,
//     CONTEXT_CLIENT_TYPE, CONTEXT_CLIENT_VERSION, CONTEXT_MAJOR_VERSION, CONTEXT_MINOR_VERSION,
//     CONTEXT_OPENGL_FORWARD_COMPATIBLE, DEFAULT_DISPLAY, EGL1_1, EGL1_5, GREEN_SIZE, HEIGHT, NONE,
//     OPENGL_API, OPENGL_ES2_BIT, OPENGL_ES_API, PBUFFER_BIT, RED_SIZE, RENDERABLE_TYPE, RGB_BUFFER,
//     SURFACE_TYPE, TRUE, WIDTH,
// };
use memfd::{FileSeal, Memfd, MemfdOptions};
use nix::{
    ioctl_read, ioctl_write_ptr,
    libc::{ftruncate, MAP_SHARED, PROT_READ, PROT_WRITE},
    sys::mman::{mmap, munmap, MapFlags, ProtFlags},
};
use sys::*;
use x264::{Colorspace, Encoder, Encoding, Preset, Setup, Tune};

// const EGL_YUV_BUFFER_EXT: Int = 0x3300;

const NUM_PROFILES: usize = 1;
const NUM_ENTRYPOINTS: usize = 1;
const NUM_ATTRIBUTES: usize = 1;
const NUM_IMAGE_FORMATS: usize = 1;
const NUM_SUBPIC_FORMATS: usize = 1;
const NUM_DISPLAY_ATTRIBUTES: usize = 1;

#[derive(Debug)]
struct Config {
    profile: VAProfile,
    entrypoint: VAEntrypoint,
    attribs: Vec<VAConfigAttrib>,
}

enum Buffer {
    Surface {
        buf: UDmabufAllocation,
        size: usize,
        map: NonNull<u8>,
    },
    VppPipelineParameterBufferType(VAProcPipelineParameterBuffer),
    CodedBufferSegment(VACodedBufferSegment),
    EncSequenceParameter(VAEncSequenceParameterBufferH264),
    EncMiscParameter(VAEncMiscParameterBuffer),
    EncSliceParameter(VAEncSliceParameterBufferH264),
    EncPictureParameter(VAEncPictureParameterBufferH264),
    Generic {
        mem_type: u32,
        data: Vec<u8>,
    },
}

impl Buffer {
    fn from_type_t<T>(size: u32, num_elements: u32, data: Option<&[u8]>) -> Result<T, VAStatus> {
        if (size as usize) < num_elements as usize * size_of::<T>() {
            return Err(VA_STATUS_ERROR_INVALID_PARAMETER);
        }

        Ok(data
            .map(|data| unsafe { (data.as_ptr() as *mut T).read() })
            .ok_or(VA_STATUS_ERROR_INVALID_PARAMETER)?)
    }

    fn from_type(
        type_: u32,
        size: u32,
        num_elements: u32,
        data: Option<&[u8]>,
    ) -> Result<Buffer, VAStatus> {
        assert_eq!(num_elements, 1); // todo!
        Ok(match type_ {
            VABufferType_VAProcPipelineParameterBufferType => {
                Buffer::VppPipelineParameterBufferType(Buffer::from_type_t::<
                    VAProcPipelineParameterBuffer,
                >(size, num_elements, data)?)
            }
            VABufferType_VAEncCodedBufferType => {
                // NOTE: this is a linked list--what's the lifetime on it???
                Buffer::CodedBufferSegment(Buffer::from_type_t::<VACodedBufferSegment>(
                    size,
                    num_elements,
                    data,
                )?)
            }
            VABufferType_VAEncSequenceParameterBufferType => Buffer::EncSequenceParameter(
                Buffer::from_type_t::<VAEncSequenceParameterBufferH264>(size, num_elements, data)?,
            ),
            VABufferType_VAEncMiscParameterBufferType => Buffer::EncMiscParameter(
                Buffer::from_type_t::<VAEncMiscParameterBuffer>(size, num_elements, data)?,
            ),
            VABufferType_VAEncSliceParameterBufferType => Buffer::EncSliceParameter(
                Buffer::from_type_t::<VAEncSliceParameterBufferH264>(size, num_elements, data)?,
            ),
            VABufferType_VAEncPictureParameterBufferType => Buffer::EncPictureParameter(
                Buffer::from_type_t::<VAEncPictureParameterBufferH264>(size, num_elements, data)?,
            ),
            _ => Buffer::Generic {
                mem_type: type_,
                data: match data {
                    Some(data) => data.to_owned(),
                    None => {
                        let mut v = Vec::new();
                        v.resize((size * num_elements) as usize, 0);
                        v
                    }
                },
            },
        })
    }

    fn from_surface(buf: UDmabufAllocation, size: usize) -> Buffer {
        let map = NonNull::new(
            unsafe {
                mmap(
                    None,
                    NonZeroUsize::new(size).unwrap(),
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_SHARED,
                    buf.memfd.as_raw_fd(),
                    0,
                )
            }
            .unwrap() as *mut u8,
        )
        .unwrap();

        Buffer::Surface { buf, size, map }
    }

    fn map(&self) -> &[u8] {
        let (ptr, size) = match self {
            Buffer::CodedBufferSegment(cs) => {
                (cs as *const _ as _, mem::size_of::<VACodedBufferSegment>())
            }
            Buffer::Surface { size, map, .. } => (map.as_ptr() as *const _, *size),
            _ => todo!(),
        };

        unsafe { slice::from_raw_parts(ptr as _, size) }
    }
    fn map_mut(&mut self) -> &mut [u8] {
        let (ptr, size) = match self {
            Buffer::CodedBufferSegment(cs) => {
                (cs as *mut _ as _, mem::size_of::<VACodedBufferSegment>())
            }
            Buffer::Surface { size, map, .. } => (map.as_ptr(), *size),
            _ => todo!(),
        };

        unsafe { slice::from_raw_parts_mut(ptr as _, size) }
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Surface { buf, size, map } => f
                .debug_struct("Surface")
                .field("buf", buf)
                .field("size", size)
                .field("map", map)
                .finish(),
            Self::VppPipelineParameterBufferType(arg0) => f
                .debug_tuple("VppPipelineParameterBufferType")
                .field(arg0)
                .finish(),
            Self::CodedBufferSegment(arg0) => {
                f.debug_tuple("CodedBufferSegment").field(arg0).finish()
            }
            Self::EncSequenceParameter(arg0) => f.debug_tuple("EncSequenceParameter").finish(),
            Self::EncMiscParameter(arg0) => f.debug_tuple("EncMiscParameter").field(arg0).finish(),
            Self::EncSliceParameter(arg0) => f.debug_tuple("EncSliceParameter").finish(),
            Self::EncPictureParameter(arg0) => f.debug_tuple("EncPictureParameter").finish(),
            Self::Generic { mem_type, data } => f
                .debug_struct("Generic")
                .field("mem_type", mem_type)
                .field("data", data)
                .finish(),
        }
    }
}

#[derive(Debug)]
struct PlaneInfo {
    pitch: usize,
    offset: usize,
}

#[derive(Debug)]
struct Surface {
    width: u32,
    height: u32,
    format: VAImageFormat,
    buffer_id: u32,
    planes: Vec<PlaneInfo>, // (pitch, offset)
}

enum ContextData {
    Enc(Option<Encoder>),
    Proc,
}

impl fmt::Debug for ContextData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enc(arg0) => f.debug_tuple("Enc").finish(),
            Self::Proc => write!(f, "Proc"),
        }
    }
}

#[derive(Debug)]
struct Context {
    render_target: Option<u32>, // surface

    config_id: u32,
    picture_width: i32,
    picture_height: i32,
    flag: i32,

    data: ContextData,
}

#[derive(Debug)]
struct Image {}

#[derive(Debug)]
struct Driver {
    // egl: DynamicInstance<EGL1_5>,
    // gles: GlFns,
    // egl_display: Display,
    // gbm: Device<OwnedFd>,
    udma: UDmaBuf,
    surfaces: Vec<Option<Surface>>,
    configs: Vec<Option<Config>>,
    contexts: Vec<Option<Context>>,
    images: Vec<Option<Image>>,
    buffers: Vec<Option<Buffer>>,
    // egl_ctx: khronos_egl::Context,
    // egl_export_dmabuf_image_mesa: unsafe extern "C" fn(display: EGLDisplay,
    //                                     image: EGLImage,
    //                                     fds: *mut c_int,
    // 			        strides: *mut c_int,
    // 				offset: *mut c_int) -> Boolean,
}

unsafe extern "C" fn terminate(ctx: VADriverContextP) -> VAStatus {
    drop(Box::from_raw((*ctx).pDriverData as *mut Driver));

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn query_config_profiles(
    ctx: VADriverContextP,
    profile_list: *mut VAProfile,
    num_profiles: *mut c_int,
) -> VAStatus {
    let profile_list = slice::from_raw_parts_mut(profile_list, NUM_PROFILES);
    // profile_list[0] = VAProfile_VAProfileH264Baseline;
    profile_list[0] = VAProfile_VAProfileH264Main;
    // profile_list[2] = VAProfile_VAProfileH264High;

    *num_profiles = 1;

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn query_config_entrypoints(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint_list: *mut VAEntrypoint,
    num_entrypoints: *mut c_int,
) -> VAStatus {
    match profile {
        VAProfile_VAProfileH264Main => {
            *entrypoint_list = VAEntrypoint_VAEntrypointEncPicture;
            *num_entrypoints = 1;
        }
        _ => {
            *num_entrypoints = 0;
        }
    }

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn query_config_attributes(
    ctx: VADriverContextP,
    config_id: VAConfigID,
    profile: *mut VAProfile,
    entrypoint: *mut VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: *mut c_int,
) -> VAStatus {
    todo!()
}

unsafe extern "C" fn create_config(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint: VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: c_int,
    config_id: *mut VAConfigID,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.create_config(
        profile,
        entrypoint,
        slice::from_raw_parts(attrib_list, num_attribs as usize),
    ) {
        Ok(cid) => {
            *config_id = cid;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_config(ctx: VADriverContextP, config_id: VAConfigID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);
    *driver.configs.get_mut(config_id as usize).unwrap() = None;

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn get_config_attributes(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint: VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: c_int,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    driver.get_config_attributes(
        profile,
        entrypoint,
        slice::from_raw_parts_mut(attrib_list, num_attribs as usize),
    );
    VA_STATUS_SUCCESS
}

unsafe extern "C" fn create_surfaces(
    ctx: VADriverContextP,
    width: c_int,
    height: c_int,
    format: c_int,
    num_surfaces: c_int,
    surfaces: *mut VASurfaceID,
) -> VAStatus {
    todo!()
}

unsafe extern "C" fn destroy_surfaces(
    ctx: VADriverContextP,
    surface_list: *mut VASurfaceID,
    num_surfaces: c_int,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.destroy_surfaces(slice::from_raw_parts(surface_list, num_surfaces as usize)) {
        Ok(_) => VA_STATUS_SUCCESS,
        Err(e) => e,
    }
}

unsafe extern "C" fn create_context(
    ctx: VADriverContextP,
    config_id: VAConfigID,
    picture_width: c_int,
    picture_height: c_int,
    flag: c_int,
    render_targets: *mut VASurfaceID,
    num_render_targets: c_int,
    context: *mut VAContextID,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.create_context(
        config_id,
        picture_width,
        picture_height,
        flag,
        slice::from_raw_parts(render_targets, num_render_targets as usize),
    ) {
        Ok(ctx) => {
            *context = ctx;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_context(ctx: VADriverContextP, context: VAContextID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.contexts.get_mut(context as usize) {
        Some(s) => {
            *s = None;
            VA_STATUS_SUCCESS
        }
        None => VA_STATUS_ERROR_INVALID_CONTEXT,
    }
}

unsafe extern "C" fn create_buffer(
    ctx: VADriverContextP,
    context: VAContextID,
    type_: VABufferType,
    size: c_uint,
    num_elements: c_uint,
    data: *mut c_void,
    buf_id: *mut VABufferID,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.create_buffer(
        context,
        type_,
        size,
        num_elements,
        if data.is_null() {
            None
        } else {
            Some(slice::from_raw_parts(
                data as _,
                (size * num_elements) as usize,
            ))
        },
    ) {
        Ok(buf) => {
            *buf_id = buf;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn map_buffer(
    ctx: VADriverContextP,
    buf_id: VABufferID,
    pbuf: *mut *mut c_void,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.buffer_mut(buf_id) {
        Ok(buf) => {
            // NOTE: this is suuuuper sketchy and probably violates aliasing rules
            // there's no way to enforce the aliasing rules though, unfortunately...altho I'm sure this could be improved
            pbuf.write(buf.map_mut().as_mut_ptr() as _);
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn unmap_buffer(ctx: VADriverContextP, buf_id: VABufferID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);
    // unmap is free in this implementation, it's just shared memory
    VA_STATUS_SUCCESS
}

unsafe extern "C" fn destroy_buffer(ctx: VADriverContextP, buffer_id: VABufferID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    if let Some(buf) = driver.buffers.get_mut(buffer_id as usize) {
        *buf = None;
        VA_STATUS_SUCCESS
    } else {
        VA_STATUS_ERROR_INVALID_BUFFER
    }
}

unsafe extern "C" fn begin_picture(
    ctx: VADriverContextP,
    context: VAContextID,
    render_target: VASurfaceID,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.context_mut(context) {
        Ok(ctx) => {
            ctx.render_target = Some(render_target);
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn render_picture(
    ctx: VADriverContextP,
    context: VAContextID,
    buffers: *mut VABufferID,
    num_buffers: c_int,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.render_picture(
        context,
        slice::from_raw_parts(buffers, num_buffers as usize),
    ) {
        Ok(_) => VA_STATUS_SUCCESS,
        Err(e) => e,
    }
}

unsafe extern "C" fn end_picture(ctx: VADriverContextP, context: VAContextID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.context_mut(context) {
        Ok(ctx) => {
            ctx.render_target = None;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn sync_surface(ctx: VADriverContextP, render_target: VASurfaceID) -> VAStatus {
    // nothing to do, all CPU
    VA_STATUS_SUCCESS
}

unsafe extern "C" fn query_image_formats(
    ctx: VADriverContextP,
    format_list: *mut VAImageFormat,
    num_formats: *mut c_int,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    *num_formats =
        driver.query_image_formats(slice::from_raw_parts_mut(format_list, NUM_IMAGE_FORMATS));

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn derive_image(
    ctx: VADriverContextP,
    surface: VASurfaceID,
    image: *mut VAImage,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.derive_image(surface) {
        Ok(i) => {
            *image = i;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_image(ctx: VADriverContextP, image: VAImageID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.images.get_mut(image as usize) {
        Some(s) => {
            *s = None;
            VA_STATUS_SUCCESS
        }
        None => VA_STATUS_ERROR_INVALID_CONTEXT,
    }
}

unsafe extern "C" fn create_surfaces2(
    ctx: VADriverContextP,
    format: c_uint,
    width: c_uint,
    height: c_uint,
    surfaces: *mut VASurfaceID,
    num_surfaces: c_uint,
    attrib_list: *mut VASurfaceAttrib,
    num_attribs: c_uint,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    driver.create_surfaces(
        format,
        width,
        height,
        slice::from_raw_parts_mut(surfaces, num_surfaces as usize),
        slice::from_raw_parts(attrib_list, num_attribs as usize),
    );

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn query_surface_attributes(
    dpy: VADriverContextP,
    config: VAConfigID,
    attrib_list: *mut VASurfaceAttrib,
    num_attribs: *mut c_uint,
) -> VAStatus {
    let driver = &mut *((*dpy).pDriverData as *mut Driver);

    match driver.query_surface_attributes(
        config,
        slice::from_raw_parts_mut(attrib_list, NUM_ATTRIBUTES),
    ) {
        Ok(num) => {
            *num_attribs = num as c_uint;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn acquire_buffer_handle(
    ctx: VADriverContextP,
    buf_id: VABufferID,
    buf_info: *mut VABufferInfo,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.acquire_buffer_handle(buf_id, (*buf_info).mem_type) {
        Ok(info) => {
            *buf_info = info;
            VA_STATUS_SUCCESS
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn release_buffer_handle(ctx: VADriverContextP, buf_id: VABufferID) -> VAStatus {
    // nothing to do (yet at least)
    VA_STATUS_SUCCESS
}

unsafe extern "C" fn export_surface_handle(
    ctx: VADriverContextP,
    surface_id: VASurfaceID,
    mem_type: u32,
    flags: u32,
    descriptor: *mut c_void,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match mem_type {
        VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2 => match driver.export_surface_handle_drm_prime(
            surface_id,
            flags,
            &mut *(descriptor as *mut VADRMPRIMESurfaceDescriptor),
        ) {
            Ok(_) => VA_STATUS_SUCCESS,
            Err(e) => e,
        },
        _ => todo!(),
    }
}

unsafe extern "C" fn sync_buffer(
    ctx: VADriverContextP,
    buf_id: VABufferID,
    timeout_ns: u64,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    driver.sync_buffer(buf_id, timeout_ns);
    VA_STATUS_SUCCESS
}

unsafe extern "C" fn vpp_query_video_proc_filter_caps(
    ctx: VADriverContextP,
    context: VAContextID,
    type_: VAProcFilterType,
    filter_caps: *mut c_void,
    num_filter_caps: *mut c_uint,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    // driver.vpp_query_video_proc_filter_caps()
    todo!()
}

unsafe extern "C" fn vpp_query_video_proc_pipeline_caps(
    ctx: VADriverContextP,
    context: VAContextID,
    filters: *mut VABufferID,
    num_filters: c_uint,
    pipeline_caps: *mut VAProcPipelineCaps,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    assert_eq!(num_filters, 0); // todo

    driver.vpp_query_video_proc_pipeline_cpas(&mut *pipeline_caps);

    VA_STATUS_SUCCESS
}

unsafe extern "C" fn unimpl() -> VAStatus {
    todo!()
}

fn align_up(p: usize, align: usize) -> usize {
    assert_eq!(align.count_ones(), 1);
    let alignm1 = align - 1;
    (p + alignm1) & !alignm1
}

#[derive(Debug)]
struct UDmaBuf {
    fd: OwnedFd,
}

#[derive(Debug)]
struct UDmabufAllocation {
    dmabuf: OwnedFd,
    memfd: Memfd,
}

impl UDmaBuf {
    fn new() -> Self {
        Self {
            fd: File::open("/dev/udmabuf").unwrap().into(),
        }
    }

    fn alloc_dmabuf(&mut self, size: usize) -> UDmabufAllocation {
        unsafe {
            let memfd = MemfdOptions::default()
                .allow_sealing(true)
                .create("memfd")
                .unwrap();

            let size_aligned = align_up(size, page_size::get());

            let res = ftruncate(memfd.as_raw_fd(), size_aligned as i64);
            assert_eq!(res, 0);
            memfd.add_seal(FileSeal::SealShrink).unwrap();

            let dmabuf_fd = udmabuf_create(
                self.fd.as_raw_fd(),
                &udmabuf_create {
                    memfd: memfd.as_raw_fd() as u32,
                    flags: UDMABUF_FLAGS_CLOEXEC as u32,
                    offset: 0,
                    size: size_aligned as u64,
                },
            )
            .unwrap();

            UDmabufAllocation {
                dmabuf: OwnedFd::from_raw_fd(dmabuf_fd),
                memfd,
            }
        }
    }
}

ioctl_write_ptr!(udmabuf_create, b'u', 0x42, udmabuf_create);

impl Driver {
    unsafe fn init_context(ctx: &mut VADriverContext) {
        dcp::initialize();
        // let drm_fd = BorrowedFd::borrow_raw(*(ctx.drm_state as *const c_int))
        //     .try_clone_to_owned()
        //     .unwrap(); // this seems to always be the case. not sure if this is documented at all
        // let gbm = Device::new(drm_fd).unwrap();

        // let egl = DynamicInstance::<EGL1_5>::load_required().unwrap();

        // let egl_export_dmabuf_image_mesa = mem::transmute(egl.get_proc_address("eglExportDMABUFImageMESA").unwrap());

        // const PLATFORM_SURFACELES_MESA: u32 = 0x31DD;
        // let egl_display = egl
        //     .get_platform_display(PLATFORM_SURFACELES_MESA, DEFAULT_DISPLAY, &[ATTRIB_NONE])
        //     .unwrap();
        // egl.initialize(egl_display).unwrap();
        // egl.bind_api(OPENGL_ES_API).unwrap();

        // // let display = egl.get_display( wayland_conn.display().).unwrap();
        // // let display = egl.get_current_display().unwrap();

        // let mut configs = Vec::new();
        // configs.reserve(1);
        // egl.choose_config(
        //     egl_display,
        //     &[
        //         SURFACE_TYPE,
        //         PBUFFER_BIT,
        //         RENDERABLE_TYPE,
        //         OPENGL_ES2_BIT,
        //         RED_SIZE,
        //         1,
        //         GREEN_SIZE,
        //         1,
        //         BLUE_SIZE,
        //         1,
        //         ALPHA_SIZE,
        //         0,
        //         NONE,
        //     ],
        //     &mut configs,
        // )
        // .unwrap();

        // let egl_ctx = egl
        //     .create_context(
        //         egl_display,
        //         configs[0],
        //         None,
        //         &[CONTEXT_CLIENT_VERSION, 3, NONE],
        //     )
        //     .unwrap();

        // egl.make_current(egl_display, None, None, Some(egl_ctx))
        //     .unwrap();

        // let gles = GlFns::load_with(|c_char_ptr| {
        //     match egl.get_proc_address(CStr::from_ptr(c_char_ptr).to_str().unwrap()) {
        //         Some(ptr) => ptr as _,
        //         None => null_mut(),
        //     }
        // });

        ctx.pDriverData = Box::into_raw(Box::new(Driver {
            // egl,
            // gles,
            // egl_display,
            // egl_ctx,
            // gbm,
            udma: UDmaBuf::new(),
            surfaces: Default::default(),
            configs: Default::default(),
            contexts: Default::default(),
            images: Default::default(),
            buffers: Default::default(),
            // egl_export_dmabuf_image_mesa,
        })) as *mut c_void;

        ctx.version_major = VA_MAJOR_VERSION as i32;
        ctx.version_minor = VA_MINOR_VERSION as i32;

        ctx.max_profiles = NUM_PROFILES as i32;
        ctx.max_entrypoints = NUM_ENTRYPOINTS as i32;
        ctx.max_attributes = NUM_ATTRIBUTES as i32;
        ctx.max_image_formats = NUM_IMAGE_FORMATS as i32;
        ctx.max_subpic_formats = NUM_SUBPIC_FORMATS as i32;
        ctx.max_display_attributes = NUM_DISPLAY_ATTRIBUTES as i32;
        ctx.str_vendor = c_str!("libva-x264").as_ptr();

        let vtable = &mut *ctx.vtable;
        vtable.vaTerminate = Some(terminate);

        vtable.vaQueryConfigProfiles = Some(query_config_profiles);
        vtable.vaQueryConfigEntrypoints = Some(query_config_entrypoints);
        vtable.vaQueryConfigAttributes = Some(query_config_attributes);
        vtable.vaCreateConfig = Some(create_config);
        vtable.vaDestroyConfig = Some(destroy_config);
        vtable.vaGetConfigAttributes = Some(get_config_attributes);
        vtable.vaCreateSurfaces = Some(create_surfaces);
        vtable.vaDestroySurfaces = Some(destroy_surfaces);
        vtable.vaCreateContext = Some(create_context);
        vtable.vaDestroyContext = Some(destroy_context);
        vtable.vaCreateBuffer = Some(create_buffer);

        (&mut vtable.vaBufferSetNumElements as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        vtable.vaMapBuffer = Some(map_buffer);
        vtable.vaUnmapBuffer = Some(unmap_buffer);
        vtable.vaDestroyBuffer = Some(destroy_buffer);
        vtable.vaBeginPicture = Some(begin_picture);
        vtable.vaRenderPicture = Some(render_picture);
        vtable.vaEndPicture = Some(end_picture);
        vtable.vaSyncSurface = Some(sync_surface);
        (&mut vtable.vaQuerySurfaceStatus as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        vtable.vaQueryImageFormats = Some(query_image_formats);
        (&mut vtable.vaCreateImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        vtable.vaDeriveImage = Some(derive_image);
        vtable.vaDestroyImage = Some(destroy_image);
        (&mut vtable.vaSetImagePalette as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaGetImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus).write(unimpl);
        (&mut vtable.vaPutImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus).write(unimpl);
        (&mut vtable.vaQuerySubpictureFormats as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaCreateSubpicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaDestroySubpicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaSetSubpictureImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaSetSubpictureChromakey as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaSetSubpictureGlobalAlpha as *mut _
            as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaAssociateSubpicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaDeassociateSubpicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaQueryDisplayAttributes as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaGetDisplayAttributes as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaSetDisplayAttributes as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);

        vtable.vaCreateSurfaces2 = Some(create_surfaces2);
        vtable.vaQuerySurfaceAttributes = Some(query_surface_attributes);
        vtable.vaAcquireBufferHandle = Some(acquire_buffer_handle);
        vtable.vaReleaseBufferHandle = Some(release_buffer_handle);
        vtable.vaExportSurfaceHandle = Some(export_surface_handle);
        vtable.vaSyncBuffer = Some(sync_buffer);

        let vtable_vpp = &mut *ctx.vtable_vpp;
        // vtable_vpp.vaQueryVideoProcFilterCaps = Some(vpp_query_video_proc_filter_caps);
        vtable_vpp.vaQueryVideoProcPipelineCaps = Some(vpp_query_video_proc_pipeline_caps);
    }

    const IMAGE_FMT_NV12: VAImageFormat = VAImageFormat {
        fourcc: VA_FOURCC_NV12,
        byte_order: VA_LSB_FIRST,
        bits_per_pixel: 12,
        depth: 0,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
        alpha_mask: 0,
        va_reserved: [0; 4],
    };
    const IMAGE_FMT_YUV420: VAImageFormat = VAImageFormat {
        fourcc: VA_FOURCC_I420,
        byte_order: VA_LSB_FIRST,
        bits_per_pixel: 12,
        depth: 0,
        red_mask: 0,
        green_mask: 0,
        blue_mask: 0,
        alpha_mask: 0,
        va_reserved: [0; 4],
    };
    const IMAGE_FMT_BGRX: VAImageFormat = VAImageFormat {
        fourcc: VA_FOURCC_BGRX,
        byte_order: VA_LSB_FIRST,
        bits_per_pixel: 32,
        depth: 0,
        red_mask: 0xff000000,
        green_mask: 0x00ff0000,
        blue_mask: 0x0000ff00,
        alpha_mask: 0x000000ff,
        va_reserved: [0; 4],
    };

    fn query_image_formats(&self, num_image_formats: &mut [VAImageFormat]) -> i32 {
        num_image_formats[0] = Driver::IMAGE_FMT_NV12;
        1
    }

    fn create_surfaces(
        &mut self,
        format: u32,
        width: u32,
        height: u32,
        surfaces: &mut [u32],
        attribs: &[VASurfaceAttrib],
    ) {
        let mut fourcc = None;
        for attrib in attribs {
            match attrib.type_ {
                VASurfaceAttribType_VASurfaceAttribPixelFormat => {
                    fourcc = Some(unsafe { attrib.value.value.i } as u32);
                }
                VASurfaceAttribType_VASurfaceAttribMemoryType => {
                    let v = unsafe { attrib.value.value.i };
                    assert_eq!(v, VA_SURFACE_ATTRIB_MEM_TYPE_VA as i32);
                }
                _ => todo!(),
            }
        }

        for s in surfaces {
            *s = self.surfaces.len() as u32;
            self.surfaces.push(Some(match format {
                VA_RT_FORMAT_RGB32 => match fourcc.unwrap() {
                    VA_FOURCC_BGRX => {
                        let stride = align_up(width as usize * 4, 512);
                        let size = stride as i64 * height as i64;

                        let buf = self.udma.alloc_dmabuf(size as usize);

                        let buffer_id = self.buffers.len() as u32;
                        self.buffers
                            .push(Some(Buffer::from_surface(buf, size as usize)));
                        Surface {
                            width,
                            height,
                            format: Driver::IMAGE_FMT_BGRX,
                            buffer_id,
                            planes: vec![PlaneInfo {
                                pitch: stride,
                                offset: 0,
                            }],
                        }
                    }
                    _ => todo!(),
                },
                VA_RT_FORMAT_YUV420 => {
                    match fourcc.unwrap() {
                        VA_FOURCC_NV12 => {
                            let stride = align_up(width as usize, 2048);
                            let size = stride as i64 * (height + (height + 1) / 2) as i64;

                            let buf = self.udma.alloc_dmabuf(size as usize);

                            let buffer_id = self.buffers.len() as u32;
                            self.buffers
                                .push(Some(Buffer::from_surface(buf, size as usize)));

                            Surface {
                                // width,
                                // height,
                                format: Driver::IMAGE_FMT_NV12,
                                buffer_id,
                                width,
                                height,
                                planes: vec![
                                    PlaneInfo {
                                        pitch: stride,
                                        offset: 0,
                                    },
                                    PlaneInfo {
                                        pitch: stride,
                                        offset: stride * height as usize,
                                    },
                                ],
                            }
                        }
                        _ => todo!(),
                    }
                }
                _ => todo!(),
            }))
        }
        // todo: attribs??
        // for s in surfaces {
        //     *s = self.surfaces.len() as u32;
        //     match format {
        //         VA_RT_FORMAT_RGB32 => {
        //             // let mut tex = 0;
        //             // unsafe {
        //             //     self.gles.GenTextures(1, &mut tex);
        //             //     self.gles.BindTexture(GL_TEXTURE_2D, tex);
        //             //     self.gles.TexImage2D(
        //             //         GL_TEXTURE_2D,
        //             //         0,
        //             //         GL_RGB as i32,
        //             //         width as i32,
        //             //         height as i32,
        //             //         0,
        //             //         GL_RGB,
        //             //         GL_UNSIGNED_BYTE,
        //             //         null(),
        //             //     )
        //             // };

        //             let mut configs = Vec::new();
        //             self.egl.choose_config(
        //                 self.egl_display,
        //                 &[
        //                     SURFACE_TYPE,
        //                     PBUFFER_BIT,
        //                     RENDERABLE_TYPE,
        //                     OPENGL_ES2_BIT,
        //                     COLOR_BUFFER_TYPE, RGB_BUFFER,
        //                     RED_SIZE, 1,
        //                     GREEN_SIZE, 1,
        //                     BLUE_SIZE, 1,
        //                     ALPHA_SIZE, 0,
        //                     // EGL_YUV_NUMBER_OF_PLANES_EXT
        //                     NONE,
        //                 ],
        //                 &mut configs,
        //             ).unwrap();

        //             // let surface = self.egl.create_pbuffer_surface(self.egl_display, configs[0], &[
        //             //     WIDTH, width as _,
        //             //     HEIGHT, height as _,
        //             //     NONE,
        //             // ]).unwrap();

        //             // self.egl.bind_tex_image(display, surface, buffer)
        //             self.egl.create_pixmap_surface(self.egl_display, configs[0], , attrib_list)
        //             let image = self.egl.create_image(self.egl_display, self.egl_ctx, GL_TEXTURE_2D, tex, &[ ATTRIB_NONE ]).unwrap();

        //             self.egl_export_dmabuf_image_mesa(self.egl_display.as_ptr(), )

        //             let buffer_id = self.buffers.len() as u32;
        //             self.buffers.push(Some(Buffer { gl_handle: tex, mem_size: 3 * width * height }));
        //             self.surfaces.push(Some(Surface {
        //                 width,
        //                 height,
        //                 format: Driver::IMAGE_FMT_RGB32,
        //                 buffer_id,
        //                 planes: vec![(width * 3, 0)], // todo: get from gl
        //             }))
        //         }
        //         VA_RT_FORMAT_YUV420 => {
        //             let mut tex = 0;
        //             unsafe {
        //                 self.gles.GenTextures(1, &mut tex);
        //             };

        //             let buffer_id = self.buffers.len() as u32;
        //             self.buffers.push(Some(Buffer { gl_handle: tex, mem_size: width * height + width * (height + 1) / 2 }));
        //             self.surfaces.push(Some(Surface {
        //                 width,
        //                 height,
        //                 format: Driver::IMAGE_FMT_NV12,
        //                 buffer_id,
        //                 planes: vec![(width, 0), (width, width * height)], // TODO: this is incorrect! get from gl!
        //             }))
        //         }
        //         _ => todo!(),
        //     }
        // }
    }

    fn create_config(
        &mut self,
        profile: i32,
        entrypoint: u32,
        attribs: &[VAConfigAttrib],
    ) -> Result<u32, VAStatus> {
        self.configs.push(Some(Config {
            profile,
            entrypoint,
            attribs: attribs.to_owned(),
        }));

        Ok((self.configs.len() - 1) as u32)
    }

    fn query_surface_attributes(
        &mut self,
        config: u32,
        num_attributes: &mut [VASurfaceAttrib],
    ) -> Result<usize, VAStatus> {
        let c = self
            .configs
            .get_mut(config as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_CONFIG)?;

        Ok(0)
    }

    fn derive_image(&mut self, surfaceid: u32) -> Result<VAImage, i32> {
        let surface = self
            .surfaces
            .get(surfaceid as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_SURFACE)?
            .as_ref()
            .ok_or(VA_STATUS_ERROR_INVALID_SURFACE)?;

        // let buffer = &self.buffer(surface.buffer_id)?.buffer;
        let buffer = self.buffer(surface.buffer_id)?;
        let (dmabuf_fd, size) = match buffer {
            Buffer::Surface { buf, size, .. } => (buf, size),
            _ => return Err(VA_STATUS_ERROR_INVALID_SURFACE),
        };

        let mut pitches = [0; 3];
        let mut offsets = [0; 3];

        for (i, PlaneInfo { pitch, offset }) in surface.planes.iter().enumerate() {
            // pitches[i] = buffer.stride_for_plane(i as i32).unwrap();
            // offsets[i] = buffer.offset(i as i32).unwrap();
            pitches[i] = *pitch as u32;
            offsets[i] = *offset as u32;
        }

        // let width = buffer.width().unwrap() as u16;
        // let height = buffer.height().unwrap() as u16;
        // let data_size =
        //     buffer.width().unwrap() * buffer.width().unwrap() * surface.format.bits_per_pixel / 8; // eeeeh this is not ideal--strides!!
        // let num_planes = buffer.plane_count().unwrap();
        let width = surface.width.try_into().unwrap();
        let height = surface.height.try_into().unwrap();
        let data_size = (*size).try_into().unwrap();
        let num_planes = surface.planes.len() as u32;

        let image_id = self.images.len() as u32;
        self.images.push(Some(Image {}));

        Ok(VAImage {
            image_id,
            format: surface.format,
            buf: surface.buffer_id,
            width,
            height,
            data_size,
            num_planes,
            pitches,
            offsets,
            num_palette_entries: 0,
            entry_bytes: 0,
            component_order: [0; 4],
            va_reserved: [0; 4],
        })
    }

    fn create_context(
        &mut self,
        config_id: u32,
        picture_width: i32,
        picture_height: i32,
        flag: i32,
        render_targets: &[u32],
    ) -> Result<u32, VAStatus> {
        let config = self.config(config_id)?;
        self.contexts.push(Some(Context {
            render_target: None,
            config_id,
            picture_width,
            picture_height,
            flag,
            data: match config.entrypoint {
                VAEntrypoint_VAEntrypointEncPicture => ContextData::Enc(None),
                VAEntrypoint_VAEntrypointVideoProc => ContextData::Proc,
                _ => todo!(),
            },
        }));
        Ok(self.contexts.len() as u32 - 1)
    }

    fn destroy_surfaces(&mut self, surfaces: &[u32]) -> Result<(), VAStatus> {
        for surf in surfaces {
            *self
                .surfaces
                .get_mut(*surf as usize)
                .ok_or(VA_STATUS_ERROR_INVALID_SURFACE)? = None;
        }
        Ok(())
    }

    fn get_config_attributes(&self, profile: i32, entrypoint: u32, configs: &mut [VAConfigAttrib]) {
        for c in configs {
            match c.type_ {
                VAConfigAttribType_VAConfigAttribRTFormat => {
                    c.value = VA_RT_FORMAT_YUV420 as u32;
                }
                VAConfigAttribType_VAConfigAttribRateControl => {
                    c.value = VA_RC_CBR as u32;
                }
                VAConfigAttribType_VAConfigAttribEncMaxRefFrames => {
                    c.value = 10; // TODO(RG)!
                }
                // TODO header stuff
                _ => {
                    c.value = VA_ATTRIB_NOT_SUPPORTED as u32;
                }
            }
        }
    }

    fn acquire_buffer_handle(&self, buf_id: u32, mem_type: u32) -> Result<VABufferInfo, i32> {
        if mem_type == VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME as u32 {
            let buffer = self
                .buffers
                .get(buf_id as usize)
                .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
                .as_ref()
                .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?;

            match buffer {
                Buffer::Surface { buf, size, .. } => {
                    Ok(VABufferInfo {
                        handle: buf.dmabuf.as_raw_fd() as usize,
                        type_: 0, // ???
                        mem_type,
                        mem_size: *size,
                        va_reserved: Default::default(),
                    })
                }
                _ => todo!(),
            }
        } else {
            todo!()
        }
    }

    fn export_surface_handle_drm_prime(
        &self,
        surface_id: u32,
        flags: u32,
        descriptor: &mut VADRMPRIMESurfaceDescriptor,
    ) -> Result<(), VAStatus> {
        let surf = self.surface(surface_id)?;
        let buffer = self.buffer(surf.buffer_id)?;

        let (buf, size) = match buffer {
            Buffer::Surface { buf, size, .. } => (buf, size),
            _ => return Err(VA_STATUS_ERROR_INVALID_SURFACE),
        };

        let mut offset = [0; 4];
        let mut pitch = [0; 4];

        for (i, plane) in surf.planes.iter().enumerate() {
            pitch[i] = plane.pitch as u32;
            offset[i] = plane.offset as u32;
        }

        *descriptor = VADRMPRIMESurfaceDescriptor {
            fourcc: surf.format.fourcc,
            width: surf.width,
            height: surf.height,
            num_objects: 1,
            objects: [
                _VADRMPRIMESurfaceDescriptor__bindgen_ty_1 {
                    fd: buf.dmabuf.as_raw_fd(),
                    size: *size as u32,
                    drm_format_modifier: 0, // ??
                },
                Default::default(),
                Default::default(),
                Default::default(),
            ],
            num_layers: 1,
            layers: [
                _VADRMPRIMESurfaceDescriptor__bindgen_ty_2 {
                    drm_format: surf.format.fourcc, // idk if this is ok
                    num_planes: surf.planes.len() as u32,
                    object_index: [0; 4],
                    offset,
                    pitch,
                },
                Default::default(),
                Default::default(),
                Default::default(),
            ],
        };
        Ok(())
    }

    fn render_picture(
        &mut self,
        context: VAContextID,
        buffers: &[VABufferID],
    ) -> Result<(), VAStatus> {
        let context = Driver::get_field_mut(&mut self.contexts, context)?;
        let config = Driver::get_field(&self.configs, context.config_id)?;

        let target = Driver::get_field(
            &self.surfaces,
            context
                .render_target
                .ok_or(VA_STATUS_ERROR_INVALID_SURFACE)?,
        )?;

        for buf in buffers {
            match (
                Driver::get_field(&self.buffers, *buf)?,
                config.profile,
                &mut context.data,
            ) {
                (Buffer::Surface { .. }, _, _) => todo!(),
                (Buffer::VppPipelineParameterBufferType(pic), _, ContextData::Proc) => {
                    assert!(pic.output_region.is_null());
                    assert_eq!(pic.num_filters, 0);
                    assert!(pic.blend_state.is_null());
                    assert!(pic.blend_state.is_null());
                    assert!(pic.additional_outputs.is_null());

                    let input_surface = Driver::get_field(&self.surfaces, pic.surface)?;

                    let surface_region = unsafe { &*pic.surface_region };
                    assert_eq!(target.width, surface_region.width as u32);
                    assert_eq!(target.height, surface_region.height as u32);
                    assert_eq!(surface_region.x, 0);
                    assert_eq!(surface_region.y, 0);
                    assert_eq!(input_surface.format.fourcc, VA_FOURCC_BGRX);
                    assert_eq!(target.format.fourcc, VA_FOURCC_NV12);

                    println!(
                        "proc rendering {} -> {}",
                        input_surface.buffer_id, target.buffer_id
                    );

                    // todo!();
                    //     // let input_map =
                    let ob = (&self.buffers[target.buffer_id as usize] as *const _) as usize;
                    let [input_buffer, output_buffer] = Driver::get_field_elements(
                        &mut self.buffers,
                        [input_surface.buffer_id, target.buffer_id],
                    )?;
                    assert_eq!(ob, output_buffer as *const _ as usize);

                    let input_map = input_buffer.map();
                    let output_map = output_buffer.map_mut();

                    let plane_delimiter = target.planes[0].pitch as usize * target.height as usize;
                    let (y, uv) = output_map.split_at_mut(plane_delimiter);
                    convert_image(
                        surface_region.width.into(),
                        surface_region.height.into(),
                        &ImageFormat {
                            pixel_format: PixelFormat::Bgra,
                            color_space: dcp::ColorSpace::Rgb,
                            num_planes: 1,
                        },
                        Some(&[input_surface.planes[0].pitch as usize]),
                        &[input_map],
                        &ImageFormat {
                            pixel_format: PixelFormat::Nv12,
                            color_space: dcp::ColorSpace::Bt601,
                            num_planes: 2,
                        },
                        Some(&[
                            target.planes[0].pitch as usize,
                            target.planes[1].pitch as usize,
                        ]),
                        &mut [y, uv],
                    )
                    .unwrap();
                }
                (Buffer::CodedBufferSegment(_), _, _) => todo!(),
                (
                    Buffer::EncSequenceParameter(spb),
                    VAProfile_VAProfileH264Main,
                    ContextData::Enc(enc),
                ) => {
                    println!("encoding -> {}", target.buffer_id);

                    let enc = if enc.is_none() {
                        let render_target = Driver::get_field(
                            &self.surfaces,
                            context
                                .render_target
                                .ok_or(VA_STATUS_ERROR_INVALID_SURFACE)?,
                        )?;

                        *enc = Some(
                            Setup::preset(Preset::Ultrafast, Tune::StillImage, false, false)
                                .bitrate(i32::try_from(spb.bits_per_second).unwrap() / 1024)
                                // .timebase(num, den)
                                .build(
                                    match render_target.format.fourcc {
                                        VA_FOURCC_NV12 => Colorspace::NV12,
                                        _ => todo!(),
                                    },
                                    render_target.width.try_into().unwrap(),
                                    render_target.height.try_into().unwrap(),
                                )
                                .map_err(|_| VA_STATUS_ERROR_ENCODING_ERROR)?,
                        );
                        enc.as_mut().unwrap()
                    } else {
                        todo!()
                    };

                    // todo!()
                }
                (Buffer::EncSequenceParameter(spb), VAProfile_VAProfileH264Main, _) => {
                    todo!()
                }
                (
                    Buffer::EncMiscParameter(emp),
                    VAProfile_VAProfileH264Main,
                    ContextData::Enc(_),
                ) => {
                    match emp.type_ {
                        VAEncMiscParameterType_VAEncMiscParameterTypeRateControl => {
                            let rc = unsafe {
                                *(emp.data.as_ptr() as *const VAEncMiscParameterRateControl)
                            };
                            // TOOD: do something wit this
                            println!("discarding ratecontrol for now");
                        }
                        VAEncMiscParameterType_VAEncMiscParameterTypeHRD => {
                            println!("discarding hrd");
                        }
                        VAEncMiscParameterType_VAEncMiscParameterTypeFrameRate => {
                            let rc = unsafe {
                                *(emp.data.as_ptr() as *const VAEncMiscParameterFrameRate)
                            };
                            // TOOD: do something wit this
                            println!("discarding framerate for now");
                        }
                        _ => todo!(),
                    }
                }
                (
                    Buffer::EncPictureParameter(eps),
                    VAProfile_VAProfileH264Main,
                    ContextData::Enc(_),
                ) => {
                    println!("discarding PictureParameter for now...")
                }
                (
                    Buffer::EncSliceParameter(esp),
                    VAProfile_VAProfileH264Main,
                    ContextData::Enc(enc),
                ) => {
                    let src_buf = Driver::get_field(&mut self.buffers, target.buffer_id)?.map();
                    let (y, uv) = src_buf.split_at(target.planes[1].offset);

                    let enc = enc.as_mut().unwrap();
                    let data = enc
                        .encode(
                            0,
                            x264::Image::new(
                                Colorspace::NV12,
                                target.width as i32,
                                target.height as i32,
                                &[
                                    x264::Plane {
                                        stride: target.planes[0].pitch as _,
                                        data: y,
                                    },
                                    x264::Plane {
                                        stride: target.planes[1].pitch as _,
                                        data: uv,
                                    },
                                ],
                            ),
                        )
                        .unwrap();

                    dbg!(data.0.entirety());
                }

                a => todo!("{a:?}"),
            }
        }

        Ok(())
    }

    fn sync_buffer(&self, buf_id: VABufferID, timeout_ns: u64) {}

    fn create_buffer(
        &mut self,
        context: u32,
        type_: u32,
        size: u32,
        num_elements: u32,
        data: Option<&[u8]>,
    ) -> Result<u32, i32> {
        let id = self.buffers.len();
        self.buffers
            .push(Some(Buffer::from_type(type_, size, num_elements, data)?));
        Ok(id as u32)
    }
}

// vpp
impl Driver {
    fn vpp_query_video_proc_pipeline_cpas(&self, pipeline_caps: &mut VAProcPipelineCaps) {
        const INPUT_COLOR_STANDARDS: &[VAProcColorStandardType] =
            &[_VAProcColorStandardType_VAProcColorStandardBT709];
        const OUTPUT_COLOR_STANDARDS: &[VAProcColorStandardType] =
            &[_VAProcColorStandardType_VAProcColorStandardBT709];
        const INPUT_PIXEL_FORMATS: &[u32] = &[VA_FOURCC_BGRX];
        const OUTPUT_PIXEL_FORMATS: &[u32] = &[VA_FOURCC_NV12];
        // https://intel.github.io/libva/structVAProcPipelineCaps.html#adca82f311a2b95bc40f799ba151db5e0
        *pipeline_caps = VAProcPipelineCaps {
            pipeline_flags: 0,
            filter_flags: 0,
            num_forward_references: 0,
            num_backward_references: 0,
            input_color_standards: INPUT_COLOR_STANDARDS.as_ptr() as _,
            num_input_color_standards: INPUT_COLOR_STANDARDS.len() as u32,
            output_color_standards: OUTPUT_COLOR_STANDARDS.as_ptr() as _,
            num_output_color_standards: OUTPUT_COLOR_STANDARDS.len() as u32,
            rotation_flags: 0,
            blend_flags: 0,
            mirror_flags: 0,
            num_additional_outputs: 0,
            num_input_pixel_formats: INPUT_PIXEL_FORMATS.len() as u32,
            input_pixel_format: INPUT_PIXEL_FORMATS.as_ptr() as _,
            num_output_pixel_formats: OUTPUT_PIXEL_FORMATS.len() as u32,
            output_pixel_format: OUTPUT_PIXEL_FORMATS.as_ptr() as _,
            max_input_width: 16384,
            max_input_height: 16384,
            min_input_width: 2,
            min_input_height: 2,
            max_output_width: 16384,
            max_output_height: 16384,
            min_output_width: 2,
            min_output_height: 2,
            va_reserved: Default::default(),
        }
    }
}

// helpers
impl Driver {
    // TODO: don't just return invalid buffer for everything lmao
    fn get_field<T>(vec: &Vec<Option<T>>, id: u32) -> Result<&T, VAStatus> {
        vec.get(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_ref()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }
    fn get_field_mut<T>(vec: &mut Vec<Option<T>>, id: u32) -> Result<&mut T, VAStatus> {
        vec.get_mut(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_mut()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }

    fn get_field_elements<T, const N: usize>(
        vec: &mut Vec<Option<T>>,
        mut indicies: [u32; N],
    ) -> Result<[&mut T; N], VAStatus> {
        // indicies.sort();
        assert!(indicies.windows(2).all(|w| w[0] < w[1])); // unimplemented

        let mut iter = vec.iter_mut();

        array::try_from_fn(|out_idx| {
            let in_idx = indicies[out_idx];
            let distance_from_last_index = usize::try_from(if out_idx == 0 {
                in_idx
            } else {
                in_idx - indicies[out_idx - 1] - 1
            })
            .unwrap();

            let r = iter
                .nth(distance_from_last_index)
                .map(|a| a.as_mut())
                .flatten();
            r
        })
        .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }

    fn buffer(&self, id: u32) -> Result<&Buffer, VAStatus> {
        Driver::get_field(&self.buffers, id)
    }
    fn buffer_mut(&mut self, id: u32) -> Result<&mut Buffer, VAStatus> {
        self.buffers
            .get_mut(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_mut()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }

    fn surface(&self, id: u32) -> Result<&Surface, VAStatus> {
        self.surfaces
            .get(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_ref()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }
    fn config(&self, id: u32) -> Result<&Config, VAStatus> {
        Driver::get_field(&self.configs, id)
    }
    fn context(&self, id: u32) -> Result<&Context, VAStatus> {
        self.contexts
            .get(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_ref()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }
    fn context_mut(&mut self, id: u32) -> Result<&mut Context, VAStatus> {
        self.contexts
            .get_mut(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)?
            .as_mut()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER)
    }
}

#[no_mangle]
extern "C" fn __vaDriverInit_1_13(ctx: VADriverContextP) -> VAStatus {
    unsafe {
        Driver::init_context(&mut *ctx);
    }

    VA_STATUS_SUCCESS
}
