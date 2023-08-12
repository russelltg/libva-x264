mod sys;

use std::ffi::c_void;

use c_string::c_str;
use sys::*;

#[derive(Debug, Default)]
struct Driver {}

pub unsafe extern "C" fn terminate(ctx: VADriverContextP) -> VAStatus {
    drop(Box::from_raw((*ctx).pDriverData as *mut Driver));

    VA_STATUS_SUCCESS as VAStatus
}

pub unsafe extern "C" fn QueryConfigProfiles(
    ctx: VADriverContextP,
    profile_list: *mut VAProfile,
    num_profiles: *mut ::std::os::raw::c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn QueryConfigEntrypoints(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint_list: *mut VAEntrypoint,
    num_entrypoints: *mut ::std::os::raw::c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn QueryConfigAttributes(
    ctx: VADriverContextP,
    config_id: VAConfigID,
    profile: *mut VAProfile,
    entrypoint: *mut VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: *mut ::std::os::raw::c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn CreateConfig(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint: VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: ::std::os::raw::c_int,
    config_id: *mut VAConfigID,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn DestroyConfig(ctx: VADriverContextP, config_id: VAConfigID) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn GetConfigAttributes(
    ctx: VADriverContextP,
    profile: VAProfile,
    entrypoint: VAEntrypoint,
    attrib_list: *mut VAConfigAttrib,
    num_attribs: ::std::os::raw::c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn CreateSurfaces(
    ctx: VADriverContextP,
    width: ::std::os::raw::c_int,
    height: ::std::os::raw::c_int,
    format: ::std::os::raw::c_int,
    num_surfaces: ::std::os::raw::c_int,
    surfaces: *mut VASurfaceID,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn DestroySurfaces(
    ctx: VADriverContextP,
    surface_list: *mut VASurfaceID,
    num_surfaces: ::std::os::raw::c_int,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn CreateContext(
    ctx: VADriverContextP,
    config_id: VAConfigID,
    picture_width: ::std::os::raw::c_int,
    picture_height: ::std::os::raw::c_int,
    flag: ::std::os::raw::c_int,
    render_targets: *mut VASurfaceID,
    num_render_targets: ::std::os::raw::c_int,
    context: *mut VAContextID,
) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn DestroyContext(ctx: VADriverContextP, context: VAContextID) -> VAStatus {
    todo!()
}

pub unsafe extern "C" fn unimpl() -> VAStatus {
    todo!()
}

impl Driver {
    unsafe fn init_context(ctx: &mut VADriverContext) {
        ctx.pDriverData = Box::into_raw(Box::new(Driver::default())) as *mut c_void;

        ctx.version_major = VA_MAJOR_VERSION as i32;
        ctx.version_minor = VA_MINOR_VERSION as i32;

        ctx.max_profiles = 1;
        ctx.max_entrypoints = 1;
        ctx.max_attributes = 1;
        ctx.max_image_formats = 1;
        ctx.max_subpic_formats = 1;
        ctx.max_display_attributes = 1;
        ctx.str_vendor = c_str!("libva-x264").as_ptr();

        let vtable = &mut *ctx.vtable;
        vtable.vaTerminate = Some(terminate);

        vtable.vaQueryConfigProfiles = Some(QueryConfigProfiles);
        vtable.vaQueryConfigEntrypoints = Some(QueryConfigEntrypoints);
        vtable.vaQueryConfigAttributes = Some(QueryConfigAttributes);
        vtable.vaCreateConfig = Some(CreateConfig);
        vtable.vaDestroyConfig = Some(DestroyConfig);
        vtable.vaGetConfigAttributes = Some(GetConfigAttributes);
        vtable.vaCreateSurfaces = Some(CreateSurfaces);
        vtable.vaDestroySurfaces = Some(DestroySurfaces);
        vtable.vaCreateContext = Some(CreateContext);
        vtable.vaDestroyContext = Some(DestroyContext);

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
        (&mut vtable.vaQueryImageFormats as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaCreateImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaDeriveImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
        (&mut vtable.vaDestroyImage as *mut _ as *mut unsafe extern "C" fn() -> VAStatus)
            .write(unimpl);
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
