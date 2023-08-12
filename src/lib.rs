mod sys;

use ::std::os::raw::{c_int, c_uint};
use core::slice;
use std::ffi::c_void;

use c_string::c_str;
use sys::*;

const NUM_PROFILES: usize = 1;
const NUM_ENTRYPOINTS: usize = 1;
const NUM_ATTRIBUTES: usize = 1;
const NUM_IMAGE_FORMATS: usize = 1;
const NUM_SUBPIC_FORMATS: usize = 1;
const NUM_DISPLAY_ATTRIBUTES: usize = 1;

#[derive(Debug)]
struct Config {}

#[derive(Debug)]
struct Surface {
    width: u32,
    height: u32,
    pitch: Vec<u32>,
    format: VAImageFormat,
    data: Vec<Vec<u8>>,
}
impl Surface {
    fn new_rgb32(width: u32, height: u32) -> Surface {
        Surface {
            width,
            height,
            pitch: vec![width * 3],
            format: Driver::IMAGE_FMT_RGB32,
            data: vec![{
                let mut v = Vec::new();
                v.resize(width as usize * height as usize * 3, 0);
                v
            }],
        }
    }

    fn new_nv12(width: u32, height: u32) -> Surface {
        Surface {
            width,
            height,
            pitch: vec![width, width],
            format: Driver::IMAGE_FMT_NV12,
            data: vec![
                {
                    let mut v = Vec::new();
                    v.resize(width as usize * height as usize, 0);
                    v
                },
                {
                    let mut v = Vec::new();
                    v.resize(width as usize * (height as usize + 1) / 2, 0);
                    v
                },
            ],
        }
    }
}

#[derive(Debug)]
struct Context {}

#[derive(Debug)]
struct Image {}

#[derive(Debug, Default)]
struct Driver {
    surfaces: Vec<Option<Surface>>,
    configs: Vec<Config>,
    contexts: Vec<Option<Context>>,
    images: Vec<Option<Image>>,
}

pub unsafe extern "C" fn terminate(ctx: VADriverContextP) -> VAStatus {
    drop(Box::from_raw((*ctx).pDriverData as *mut Driver));

    VA_STATUS_SUCCESS as VAStatus
}

pub unsafe extern "C" fn query_config_profiles(
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

pub unsafe extern "C" fn query_config_entrypoints(
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

pub unsafe extern "C" fn query_config_attributes(
    ctx: VADriverContextP,
    config_id: VAConfigID,
    profile: *mut VAProfile,
    entrypoint: *mut VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: *mut c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn create_config(
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

pub unsafe extern "C" fn destroy_config(ctx: VADriverContextP, config_id: VAConfigID) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn get_config_attributes(
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

pub unsafe extern "C" fn create_surfaces(
    ctx: VADriverContextP,
    width: c_int,
    height: c_int,
    format: c_int,
    num_surfaces: c_int,
    surfaces: *mut VASurfaceID,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn destroy_surfaces(
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

pub unsafe extern "C" fn create_context(
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

pub unsafe extern "C" fn destroy_context(ctx: VADriverContextP, context: VAContextID) -> VAStatus {
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

pub unsafe extern "C" fn derive_image(
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

pub unsafe extern "C" fn create_surfaces2(
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

pub unsafe extern "C" fn query_surface_attributes(
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

pub unsafe extern "C" fn unimpl() -> VAStatus {
    todo!()
}

impl Driver {
    unsafe fn init_context(ctx: &mut VADriverContext) {
        ctx.pDriverData = Box::into_raw(Box::new(Driver::default())) as *mut c_void;

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
        // todo: attribs??
        for s in surfaces {
            *s = self.surfaces.len() as u32;
            match format {
                VA_RT_FORMAT_RGB32 => self.surfaces.push(Some(Surface::new_rgb32(width, height))),
                VA_RT_FORMAT_YUV420 => self.surfaces.push(Some(Surface::new_nv12(width, height))),
                _ => todo!(),
            }
        }
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

        let mut pitches = [0; 3];
        for (i, p) in surface.pitch.iter().enumerate() {
            pitches[i] = *p;
        }

        let image_id = self.images.len() as u32;
        self.images.push(Some(Image {}));

        Ok(VAImage {
            image_id,
            format: surface.format,
            buf: surfaceid, // ??
            width: surface.width as u16,
            height: surface.height as u16,
            data_size: surface.width * surface.height * surface.format.bits_per_pixel / 8,
            num_planes: surface.pitch.len() as u32,
            pitches,
            offsets: [0; 3],
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
        if mem_type == VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME {
            
        }
        todo!()
        // Ok(VABufferInfo {
        //     handle: {},
        //     type_: (),
        //     mem_type: (),
        //     mem_size: (),
        //     va_reserved: (),
        // })
    }
}

#[no_mangle]
extern "C" fn __vaDriverInit_1_13(ctx: VADriverContextP) -> VAStatus {
    unsafe {
        Driver::init_context(&mut *ctx);
    }

    VA_STATUS_SUCCESS as VAStatus
}

// fn __vaInit() { bidnings
// }
