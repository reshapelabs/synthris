use anyhow::Result;
use image::RgbImage;

pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    encode_with_selected_backend(img, quality)
}

fn encode_with_image(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut bytes =
        Vec::with_capacity((img.width() as usize * img.height() as usize / 4).max(16 * 1024));
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder.encode_image(img)?;
    Ok(bytes)
}

#[cfg(feature = "turbojpeg")]
fn encode_with_selected_backend(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let source = turbojpeg::Image {
        pixels: &img.as_raw()[..],
        width: img.width() as usize,
        pitch: img.width() as usize * 3,
        height: img.height() as usize,
        format: turbojpeg::PixelFormat::RGB,
    };
    let out = turbojpeg::compress(source, quality as i32, turbojpeg::Subsamp::Sub2x2)?;
    Ok(out.as_ref().to_vec())
}

#[cfg(not(feature = "turbojpeg"))]
fn encode_with_selected_backend(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    encode_with_image(img, quality)
}

pub fn resize_rgb(
    src: &RgbImage,
    width: u32,
    height: u32,
    filter: image::imageops::FilterType,
) -> RgbImage {
    image::imageops::resize(src, width, height, filter)
}
