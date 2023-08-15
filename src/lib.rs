mod sys;

use ::std::os::raw::{c_int, c_uint};
use core::slice;
use std::{
    ffi::{c_void, CStr},
    mem,
    os::fd::{BorrowedFd, OwnedFd, AsFd, AsRawFd, FromRawFd},
    ptr::{null, null_mut}, fs::File,
};

use c_string::c_str;
use gbm::{BufferObject, BufferObjectFlags, Device};
use gles30::{GlFns, GL_RGB, GL_TEXTURE_2D, GL_UNSIGNED_BYTE};
use khronos_egl::{
    Boolean, Display, Dynamic, DynamicInstance, EGLDisplay, EGLImage, Instance, Int,
    NativeDisplayType, Upcast, ALPHA_SIZE, ATTRIB_NONE, BLUE_SIZE, COLOR_BUFFER_TYPE,
    CONTEXT_CLIENT_TYPE, CONTEXT_CLIENT_VERSION, CONTEXT_MAJOR_VERSION, CONTEXT_MINOR_VERSION,
    CONTEXT_OPENGL_FORWARD_COMPATIBLE, DEFAULT_DISPLAY, EGL1_1, EGL1_5, GREEN_SIZE, HEIGHT, NONE,
    OPENGL_API, OPENGL_ES2_BIT, OPENGL_ES_API, PBUFFER_BIT, RED_SIZE, RENDERABLE_TYPE, RGB_BUFFER,
    SURFACE_TYPE, TRUE, WIDTH,
};
use memfd::{MemfdOptions, FileSeal};
use nix::{ioctl_read, ioctl_write_ptr, libc::ftruncate};
use sys::*;

const EGL_YUV_BUFFER_EXT: Int = 0x3300;

const NUM_PROFILES: usize = 1;
const NUM_ENTRYPOINTS: usize = 1;
const NUM_ATTRIBUTES: usize = 1;
const NUM_IMAGE_FORMATS: usize = 1;
const NUM_SUBPIC_FORMATS: usize = 1;
const NUM_DISPLAY_ATTRIBUTES: usize = 1;

#[derive(Debug)]
struct Config {}

#[derive(Debug)]
struct Buffer {
    // gl_handle: u32,
    // egl_image: Image,
    dmabuf_fd: OwnedFd,
    size: usize,

    // buffer: BufferObject<()>,
}

#[derive(Debug)]
struct Surface {
    width: u32,
    height: u32,
    format: VAImageFormat,
    buffer_id: u32,
    planes: Vec<(u32, u32)>, // (pitch, offset)
}

#[derive(Debug)]
struct Context {}

#[derive(Debug)]
struct Image {}

#[derive(Debug)]
struct Driver {
    // egl: DynamicInstance<EGL1_5>,
    // gles: GlFns,
    // egl_display: Display,
    // gbm: Device<OwnedFd>,
    surfaces: Vec<Option<Surface>>,
    configs: Vec<Config>,
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

    VA_STATUS_SUCCESS as VAStatus
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

    VA_STATUS_SUCCESS as VAStatus
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

    VA_STATUS_SUCCESS as VAStatus
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
            VA_STATUS_SUCCESS as VAStatus
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_config(ctx: VADriverContextP, config_id: VAConfigID) -> VAStatus {
    todo!()
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
    VA_STATUS_SUCCESS as VAStatus
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
        Ok(_) => VA_STATUS_SUCCESS as VAStatus,
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
            VA_STATUS_SUCCESS as VAStatus
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_context(ctx: VADriverContextP, context: VAContextID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.contexts.get_mut(context as usize) {
        Some(s) => {
            *s = None;
            VA_STATUS_SUCCESS as VAStatus
        }
        None => VA_STATUS_ERROR_INVALID_CONTEXT as VAStatus,
    }
}

unsafe extern "C" fn query_image_formats(
    ctx: VADriverContextP,
    format_list: *mut VAImageFormat,
    num_formats: *mut c_int,
) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    *num_formats =
        driver.query_image_formats(slice::from_raw_parts_mut(format_list, NUM_IMAGE_FORMATS));

    VA_STATUS_SUCCESS as VAStatus
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
            VA_STATUS_SUCCESS as VAStatus
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn destroy_image(ctx: VADriverContextP, image: VAImageID) -> VAStatus {
    let driver = &mut *((*ctx).pDriverData as *mut Driver);

    match driver.images.get_mut(image as usize) {
        Some(s) => {
            *s = None;
            VA_STATUS_SUCCESS as VAStatus
        }
        None => VA_STATUS_ERROR_INVALID_CONTEXT as VAStatus,
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

    VA_STATUS_SUCCESS as VAStatus
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
            VA_STATUS_SUCCESS as VAStatus
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
            VA_STATUS_SUCCESS as VAStatus
        }
        Err(e) => e,
    }
}

unsafe extern "C" fn unimpl() -> VAStatus {
    todo!()
}

ioctl_write_ptr!(udmabuf_create, b'k', 0x42, udmabuf_create);

impl Driver {
    unsafe fn init_context(ctx: &mut VADriverContext) {
        let udma: OwnedFd = File::open("/dev/udmabuf").unwrap().into();



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

        (&mut vtable.vaCreateBuffer as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaBufferSetNumElements as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaMapBuffer as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaUnmapBuffer as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaDestroyBuffer as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaBeginPicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaRenderPicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaEndPicture as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaSyncSurface as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
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
    const IMAGE_FMT_RGB32: VAImageFormat = VAImageFormat {
        fourcc: VA_FOURCC_RGBX,
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
        for s in surfaces {
            *s = self.surfaces.len() as u32;
            match format {
                VA_RT_FORMAT_RGB32 => {

                    let udma: OwnedFd = File::open("/dev/udmabuf").unwrap().into();
                    
                    let stride = width as usize * 3;
                    let size = stride as i64 * height as i64;
                    
                    let memfd = MemfdOptions::default().allow_sealing(true).create("memfd").unwrap();
                    
                    let dmabuf_fd = unsafe { 
                        ftruncate(memfd.as_raw_fd(), size);
                        memfd.add_seal(FileSeal::SealShrink);
                         OwnedFd::from_raw_fd(udmabuf_create(udma.as_raw_fd(), &udmabuf_create {
                        memfd: memfd.as_raw_fd() as u32,
                        flags: UDMABUF_FLAGS_CLOEXEC,
                        offset: 0,
                        size: size as u64,
                    }).unwrap()) };

                    let buffer_id = self.buffers.len() as u32;
                    self.buffers.push(Some(Buffer { dmabuf_fd, size: size as usize }));

                    self.surfaces.push(Some(Surface {
                        // width,
                        // height,
                        format: Driver::IMAGE_FMT_RGB32,
                        buffer_id,
                        width,
                        height,
                        planes: vec![(stride.try_into().unwrap(), 0)]
                        // planes: vec![(width * 3, 0)], // todo: get from gl
                    }))


                    // let buffer = self
                    // let mut format = None;
                    // // let mut usage = BufferObjectFlags::WRITE | BufferObjectFlags::SCANOUT; // uhhh idk why i can't set write, but maybe that's ok?
                    // let mut usage = BufferObjectFlags::SCANOUT;

                    // for attrib in attribs {
                    //     match attrib.type_ {
                    //         VASurfaceAttribType_VASurfaceAttribPixelFormat => {
                    //             match unsafe { attrib.value.value.i } as u32 {
                    //                 // VA_FOURCC_BGRX => format = Some(gbm::Format::Bgrx8888),
                    //                 VA_FOURCC_BGRX => format = Some(gbm::Format::Xbgr8888), // buffer creation fails unless it's xbgr8888
                    //                 _ => todo!(),
                    //             }
                    //         }
                    //         VASurfaceAttribType_VASurfaceAttribMemoryType => {
                    //             let v = unsafe { attrib.value.value.i };
                    //             assert_eq!(v, VA_SURFACE_ATTRIB_MEM_TYPE_VA as i32);
                    //         }
                    //         _ => todo!(),
                    //     }
                    // }
                    //     .gbm
                    //     .create_buffer_object::<()>(width, height, format.unwrap(), usage)
                    //     .unwrap();

                    // let buffer_id = self.buffers.len() as u32;
                    // self.buffers.push(Some(Buffer { buffer }));

                    // self.surfaces.push(Some(Surface {
                    //     // width,
                    //     // height,
                    //     format: Driver::IMAGE_FMT_RGB32,
                    //     buffer_id,
                    //     // planes: vec![(width * 3, 0)], // todo: get from gl
                    // }))
                }
                _ => todo!(),
            }
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
        todo!()
    }

    fn create_config(
        &mut self,
        profile: i32,
        entrypoint: u32,
        attribs: &[VAConfigAttrib],
    ) -> Result<u32, VAStatus> {
        self.configs.push(Config {});

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
            .ok_or(VA_STATUS_ERROR_INVALID_CONFIG as VAStatus)?;

        Ok(0)
    }

    fn derive_image(&mut self, surfaceid: u32) -> Result<VAImage, i32> {
        let surface = self
            .surfaces
            .get(surfaceid as usize)
            .ok_or(VA_INVALID_SURFACE as VAStatus)?
            .as_ref()
            .ok_or(VA_INVALID_SURFACE as VAStatus)?;

        // let buffer = &self.buffer(surface.buffer_id)?.buffer;
        let buffer = &self.buffer(surface.buffer_id)?;

        let mut pitches = [0; 3];
        let mut offsets = [0; 3];

        for (i, (stride, offset)) in surface.planes.iter().enumerate() {
            // pitches[i] = buffer.stride_for_plane(i as i32).unwrap();
            // offsets[i] = buffer.offset(i as i32).unwrap();
            pitches[i] = *stride as u32;
            offsets[i] = *offset as u32;

        }

        // let width = buffer.width().unwrap() as u16;
        // let height = buffer.height().unwrap() as u16;
        // let data_size =
        //     buffer.width().unwrap() * buffer.width().unwrap() * surface.format.bits_per_pixel / 8; // eeeeh this is not ideal--strides!!
        // let num_planes = buffer.plane_count().unwrap();
        let width = surface.width.try_into().unwrap();
        let height = surface.height.try_into().unwrap();
        let data_size = buffer.size.try_into().unwrap();
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
        self.contexts.push(Some(Context {}));
        Ok(self.contexts.len() as u32 - 1)
    }

    fn destroy_surfaces(&mut self, surfaces: &[u32]) -> Result<(), VAStatus> {
        for surf in surfaces {
            *self
                .surfaces
                .get_mut(*surf as usize)
                .ok_or(VA_INVALID_SURFACE as VAStatus)? = None;
        }
        Ok(())
    }

    fn get_config_attributes(&self, profile: i32, entrypoint: u32, configs: &mut [VAConfigAttrib]) {
        for c in configs {
            match c.type_ {
                VAConfigAttribType_VAConfigAttribRTFormat => {
                    c.value = VA_RT_FORMAT_YUV420;
                }
                VAConfigAttribType_VAConfigAttribRateControl => {
                    c.value = VA_RC_CBR;
                }
                VAConfigAttribType_VAConfigAttribEncMaxRefFrames => {
                    c.value = 10; // TODO(RG)!
                }
                // TODO header stuff
                _ => {
                    c.value = VA_ATTRIB_NOT_SUPPORTED;
                }
            }
        }
    }

    fn acquire_buffer_handle(&self, buf_id: u32, mem_type: u32) -> Result<VABufferInfo, i32> {
        // if mem_type == VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME {
        //     let buffer = self
        //         .buffers
        //         .get(buf_id as usize)
        //         .ok_or(VA_STATUS_ERROR_INVALID_BUFFER as VAStatus)?
        //         .as_ref()
        //         .ok_or(VA_STATUS_ERROR_INVALID_BUFFER as VAStatus)?;

        //     return Ok(VABufferInfo {
        //         handle: buffer.gl_handle as usize,
        //         type_: 0, // ???
        //         mem_type,
        //         mem_size, buffer.mem_size,
        //         va_reserved: [0; _],
        //     })
        // }
        todo!()
    }

    fn buffer(&self, id: u32) -> Result<&Buffer, VAStatus> {
        self.buffers
            .get(id as usize)
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER as VAStatus)?
            .as_ref()
            .ok_or(VA_STATUS_ERROR_INVALID_BUFFER as VAStatus)
    }
}

#[no_mangle]
extern "C" fn __vaDriverInit_1_13(ctx: VADriverContextP) -> VAStatus {
    unsafe {
        Driver::init_context(&mut *ctx);
    }

    VA_STATUS_SUCCESS as VAStatus
}
