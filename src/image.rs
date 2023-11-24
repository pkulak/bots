use bytes::Bytes;
use exif::{In, Tag};
use image::imageops::FilterType;
use image::io::Reader as ImageReader;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::io::Cursor;

use libheif_rs::{ColorSpace, HeifContext, RgbChroma};

extern crate image;

pub fn convert_heic_to_jpeg(image: &Bytes) -> anyhow::Result<Bytes> {
    println!("decoding HEIC");

    let ctx = HeifContext::read_from_bytes(image)?;
    let handle = ctx.primary_image_handle()?;
    let decoded = handle.decode(ColorSpace::Rgb(RgbChroma::Rgb), false)?;
    let data = Bytes::copy_from_slice(decoded.planes().interleaved.unwrap().data);

    shrink_to_jpeg(&data, handle.width(), handle.height())
}

pub fn shrink_jpeg(image: &Bytes) -> anyhow::Result<Bytes> {
    let mut decoded = ImageReader::new(Cursor::new(image.to_vec()))
        .with_guessed_format()?
        .decode()?;

    // rotate, if needed
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(image.to_vec())) {
        if let Some(orientation) = exif.get_field(Tag::Orientation, In::PRIMARY) {
            if let Some(o) = orientation.value.get_uint(0) {
                println!("Orientation: {}", o);

                match o {
                    1 => {} // correct
                    2 => {
                        decoded = decoded.flipv();
                    }
                    3 => {
                        decoded = decoded.rotate180();
                    }
                    4 => {
                        decoded = decoded.fliph();
                    }
                    5 => {
                        decoded = decoded.rotate90();
                        decoded = decoded.flipv();
                    }
                    6 => {
                        decoded = decoded.rotate90();
                    }
                    7 => {
                        decoded = decoded.rotate270();
                        decoded = decoded.flipv();
                    }
                    8 => {
                        decoded = decoded.rotate270();
                    }
                    _ => {}
                };
            }
        }
    }

    let width = decoded.width();
    let height = decoded.height();

    shrink_to_jpeg(&Bytes::from(decoded.into_bytes()), width, height)
}

const WIDTH: u32 = 2560;
const HEIGHT: u32 = 1600;

pub fn shrink_to_jpeg(img: &Bytes, width: u32, height: u32) -> anyhow::Result<Bytes> {
    println!("resizing");

    let buffer = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(width, height, img.to_vec()).unwrap();
    let image = DynamicImage::from(buffer);

    let resized = if width > WIDTH || height > HEIGHT {
        image.resize(WIDTH, HEIGHT, FilterType::Lanczos3)
    } else {
        image
    };

    println!("encoding as JPEG");

    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);

    comp.set_size(resized.width() as usize, resized.height() as usize);
    comp.set_mem_dest();
    comp.start_compress();

    comp.write_scanlines(resized.as_bytes());

    comp.finish_compress();

    Ok(Bytes::from(comp.data_to_vec().unwrap()))
}
