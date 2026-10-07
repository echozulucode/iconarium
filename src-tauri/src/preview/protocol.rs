//! Custom URI protocols.
//!
//! * `thumb://localhost/{id}/{fingerprint}` — cached PNG thumbnail; a miss enqueues a P0
//!   render and the response is sent when it completes. Only mounted `<img>` elements
//!   request thumbnails, so visibility drives priority naturally.
//! * `svgfile://localhost/{id}/{fingerprint}` — the full SVG for the viewer, normalized so
//!   the root always has a viewBox (linear mapping from doc box to the image box).
//!
//! Query strings are ignored (the UI appends `?r=n` to retry).

use super::queue::{JobFlags, P0};
use super::worker::{error_response, png_response};
use crate::app::AppCore;
use std::sync::Arc;
use svg_core::model::{AssetId, ProcessingState};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder};

fn parse_id(req: &Request<Vec<u8>>) -> Option<AssetId> {
    req.uri().path().trim_start_matches('/').split('/').next()?.parse().ok()
}

fn core_of<R: tauri::Runtime>(ctx: &UriSchemeContext<'_, R>) -> Option<Arc<AppCore>> {
    ctx.app_handle().try_state::<Arc<AppCore>>().map(|s| s.inner().clone())
}

pub fn thumb_handler<R: tauri::Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let (Some(core), Some(id)) = (core_of(&ctx), parse_id(&req)) else {
        responder.respond(error_response(StatusCode::BAD_REQUEST, "bad thumbnail request"));
        return;
    };
    tauri::async_runtime::spawn_blocking(move || serve_thumb(&core, id, responder));
}

fn serve_thumb(core: &AppCore, id: AssetId, responder: UriSchemeResponder) {
    let Some(rec) = core.record(id) else {
        responder.respond(error_response(StatusCode::NOT_FOUND, "unknown asset"));
        return;
    };
    match rec.state {
        ProcessingState::Ready => {
            let size = core.settings.read().thumbnail_size;
            let path = super::cache::thumb_path(&core.paths.thumbs_dir, id, &rec.fast_fingerprint, size);
            if let Ok(png) = std::fs::read(&path) {
                responder.respond(png_response(png));
                return;
            }
            core.waiters.lock().entry(id).or_default().push(responder);
            core.queue.push(id, P0, JobFlags::THUMB);
        }
        ProcessingState::Discovered => {
            core.waiters.lock().entry(id).or_default().push(responder);
            core.queue.push(id, P0, JobFlags::BOTH);
        }
        other => responder.respond(error_response(StatusCode::UNPROCESSABLE_ENTITY, other.as_str())),
    }
}

pub fn svgfile_handler<R: tauri::Runtime>(ctx: UriSchemeContext<'_, R>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let (Some(core), Some(id)) = (core_of(&ctx), parse_id(&req)) else {
        responder.respond(error_response(StatusCode::BAD_REQUEST, "bad svg request"));
        return;
    };
    tauri::async_runtime::spawn_blocking(move || {
        let resp = match serve_svg(&core, id) {
            Ok(r) => r,
            Err(e) => error_response(
                if e.kind == "not_found" { StatusCode::NOT_FOUND } else { StatusCode::UNPROCESSABLE_ENTITY },
                &e.message,
            ),
        };
        responder.respond(resp);
    });
}

fn serve_svg(core: &AppCore, id: AssetId) -> Result<Response<Vec<u8>>, crate::error::CmdError> {
    let (rec, meta) = core.svg_meta(id)?;
    if !rec.state.renderable() {
        return Err(crate::error::CmdError::new(
            "limit_exceeded",
            rec.parse_error.unwrap_or_else(|| "Preview unavailable".into()),
        ));
    }
    let path = core.absolute_path(&rec).ok_or_else(crate::error::CmdError::no_library)?;
    let bytes = core.read_asset_bytes(&rec, &path)?;
    let body = svg_core::svg::normalize::prepare_for_viewer(&bytes, &meta).into_owned();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/svg+xml")
        .header(header::CACHE_CONTROL, "max-age=600")
        .body(body)
        .unwrap())
}
