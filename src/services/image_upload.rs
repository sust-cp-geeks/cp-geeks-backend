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
fn detect_format(bytes: &[u8], field: &str) -> Result<ImageFormat, AppError> {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => Ok(ImageFormat::Jpeg),
        Ok(ImageFormat::Png) => Ok(ImageFormat::Png),
        Ok(ImageFormat::WebP) => Ok(ImageFormat::WebP),
        // both sides arrive together, so the message has to say which one
        _ => Err(AppError::BadRequest(format!(
            "{} is not a supported image type — use JPEG, PNG or WebP",
            field
        ))),
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

    let format = detect_format(bytes, field)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, ImageEncoder};

    // builds a real jpeg of the given size, so these tests do not depend on
    // fixture files on disk
    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let buf = image::RgbImage::from_pixel(width, height, image::Rgb([40, 90, 160]));
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 90)
            .write_image(&buf, width, height, ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let buf = image::RgbImage::from_pixel(width, height, image::Rgb([10, 200, 30]));
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&buf, width, height, ExtendedColorType::Rgb8)
            .unwrap();
        out
    }

    #[test]
    fn oversized_photos_are_shrunk_to_the_long_edge() {
        let out = process(&jpeg(4000, 3000), "ID card front").unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!(decoded.width().max(decoded.height()), MAX_DIMENSION);
        // 4:3 stays 4:3
        assert_eq!(decoded.width(), 1600);
        assert_eq!(decoded.height(), 1200);
        assert!(
            out.len() < 4000 * 3000,
            "should be far smaller than the source"
        );
    }

    #[test]
    fn small_images_are_not_scaled_up() {
        let out = process(&jpeg(600, 400), "ID card front").unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (600, 400));
    }

    #[test]
    fn output_is_always_jpeg_whatever_went_in() {
        // a png in still comes back as jpeg, which is what strips metadata
        let out = process(&png(800, 600), "ID card back").unwrap();
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Jpeg);
        assert_eq!(STORED_CONTENT_TYPE, "image/jpeg");
    }

    #[test]
    fn non_images_are_rejected_on_their_signature() {
        // a shell script named .jpg must not get through — the filename and the
        // client's content-type are never trusted
        let script = b"#!/bin/sh\necho pwned\n".to_vec();
        assert!(process(&script, "ID card front").is_err());
        // valid header, truncated body
        let mut truncated = jpeg(200, 200);
        truncated.truncate(20);
        assert!(process(&truncated, "ID card front").is_err());
    }

    #[test]
    fn empty_and_oversized_uploads_are_rejected() {
        assert!(process(&[], "ID card front").is_err());
        assert!(process(&vec![0u8; MAX_UPLOAD_BYTES + 1], "ID card front").is_err());
    }

    #[test]
    fn the_error_names_the_field_that_was_wrong() {
        // both sides are uploaded together, so the message has to say which one
        let err = process(b"not an image", "ID card back").unwrap_err();
        assert!(format!("{err:?}").contains("ID card back"));
    }

    #[test]
    fn object_keys_are_unguessable_and_paired() {
        let (front, back) = new_object_keys();
        assert!(front.ends_with("/front.jpg"));
        assert!(back.ends_with("/back.jpg"));
        // same folder for a pair
        assert_eq!(
            front.rsplit_once('/').unwrap().0,
            back.rsplit_once('/').unwrap().0
        );
        // not derived from a user id, so they cannot be walked
        let (other_front, _) = new_object_keys();
        assert_ne!(front, other_front);
    }
}
