use std::io::Cursor;
use bytes::Bytes;
use image::{DynamicImage, ImageBuffer, Rgb};
use image::imageops::FilterType;
use image::io::Reader as ImageReader;

use libheif_rs::{ColorSpace, HeifContext, RgbChroma};

extern crate image;

pub fn convert_heic_to_jpeg(image: &Bytes) -> anyhow::Result<Bytes> {
    println!("decoding HEIC");

    let ctx = HeifContext::read_from_bytes(image)?;
    let handle = ctx.primary_image_handle()?;
    let decoded = handle.decode(ColorSpace::Rgb(RgbChroma::Rgb), false)?;
    let data = Bytes::copy_from_slice(decoded.planes().interleaved.unwrap().data);

    Ok(shrink_to_jpeg(&data, handle.width(), handle.height())?)
}

pub fn shrink_jpeg(image: &Bytes) -> anyhow::Result<Bytes> {
    let decoded = ImageReader::new(Cursor::new(image.to_vec())).with_guessed_format()?.decode()?;
    shrink_to_jpeg(&Bytes::copy_from_slice(decoded.as_bytes()), decoded.width(), decoded.height())
}

pub fn shrink_to_jpeg(img: &Bytes, width: u32, height: u32) -> anyhow::Result<Bytes> {
    println!("resizing");

    let buffer = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(width, height, img.to_vec()).unwrap();
    let image = DynamicImage::from(buffer).resize(1280, 800, FilterType::Lanczos3);

    println!("encoding as JPEG");

    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);

    comp.set_size(image.width() as usize, image.height() as usize);
    comp.set_mem_dest();
    comp.start_compress();

    comp.write_scanlines(image.as_bytes());

    comp.finish_compress();

    Ok(Bytes::from(comp.data_to_vec().unwrap()))
}