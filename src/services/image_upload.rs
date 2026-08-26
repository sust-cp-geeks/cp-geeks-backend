use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{ImageFormat, ImageReader};

use crate::errors::AppError;

// biggest upload we accept before any processing
pub const MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

// longest edge after resizing — plenty to read a student id, far smaller than
// what a phone camera produces
const MAX_DIMENSION: u32 = 1600;

// jpeg quality for the stored copy
const JPEG_QUALITY: u8 = 80;

pub const STORED_CONTENT_TYPE: &str = "image/jpeg";

// checks the real file signature rather than trusting the client's content-type
// or the filename extension
fn detect_format(bytes: &[u8]) -> Result<ImageFormat, AppError> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => Ok(ImageFormat::Jpeg),
        Ok(ImageFormat::Png) => Ok(ImageFormat::Png),
        Ok(ImageFormat::WebP) => Ok(ImageFormat::WebP),
        _ => Err(AppError::BadRequest(
            "Unsupported image type — use JPEG, PNG or WebP".to_string(),
        )),
    }
}

// validates, shrinks and re-encodes an uploaded id card
//
// decoding and re-encoding is what removes EXIF, which matters because phone
// photos carry gps coordinates of wherever the picture was taken
pub fn process(bytes: &[u8], field: &str) -> Result<Vec<u8>, AppError> {
    if bytes.is_empty() {
        return Err(AppError::BadRequest(format!("{} is empty", field)));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(AppError::BadRequest(format!(
            "{} is too large — maximum {} MB",
            field,
            MAX_UPLOAD_BYTES / (1024 * 1024)
        )));
    }

    let format = detect_format(bytes)?;

    // decoding also proves it's a real image and not a renamed file
    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    let image = reader.decode().map_err(|e| {
        tracing::warn!("rejected {}: could not decode: {}", field, e);
        AppError::BadRequest(format!("{} is not a readable image", field))
    })?;

    let image = if image.width() > MAX_DIMENSION || image.height() > MAX_DIMENSION {
        image.thumbnail(MAX_DIMENSION, MAX_DIMENSION)
    } else {
        image
    };

    // drop any alpha channel — jpeg can't store it
    let rgb = image.to_rgb8();

    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(
            &rgb,
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| {
            tracing::error!("failed to encode {}: {}", field, e);
            AppError::InternalError("Failed to process image".to_string())
        })?;

    tracing::info!(
        "processed {}: {} KB -> {} KB ({}x{})",
        field,
        bytes.len() / 1024,
        out.len() / 1024,
        rgb.width(),
        rgb.height()
    );

    Ok(out)
}

// object keys for one id card pair, under a random folder
//
// deliberately not derived from the user id: the upload has to happen before
// the account row exists, and a random folder also can't be guessed by walking
// user ids
pub fn new_object_keys() -> (String, String) {
    let folder = uuid::Uuid::new_v4();
    (
        format!("{}/front.jpg", folder),
        format!("{}/back.jpg", folder),
    )
}
