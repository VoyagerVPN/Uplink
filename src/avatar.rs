use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, RgbaImage, imageops};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::{
    io::{self, Cursor},
    path::{Path, PathBuf},
};

fn surface(img: &RgbaImage) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        img.as_raw(),
        img.width(),
        img.height(),
    ))
}

pub fn stored(value: &str) -> Option<Image> {
    if value.is_empty() {
        return None;
    }
    let decoded = image::load_from_memory(&STANDARD.decode(value).ok()?).ok()?;
    Some(surface(&decoded.into_rgba8()))
}

fn invitation_pixels(url: &str) -> Option<image::GrayImage> {
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<image::Luma<u8>>()
            .module_dimensions(4, 4)
            .build(),
    )
}

pub fn invitation_qr(url: &str) -> Image {
    invitation_pixels(url)
        .map(|image| surface(&image::DynamicImage::ImageLuma8(image).into_rgba8()))
        .unwrap_or_default()
}

pub fn qr(url: &str) -> Image {
    use uplink::invite::*;
    let data: &[u8] = match url {
        COOP => include_bytes!("../assets/invites/qr-coop.png"),
        ARENA => include_bytes!("../assets/invites/qr-arena.png"),
        PINE => include_bytes!("../assets/invites/qr-pine.png"),
        NIGHT => include_bytes!("../assets/invites/qr-night.png"),
        _ => include_bytes!("../assets/invites/qr-after.png"),
    };
    surface(
        &image::load_from_memory(data)
            .expect("bundled QR")
            .into_rgba8(),
    )
}

pub fn pick() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let mut buf = vec![0u16; 32768];
    let filter: Vec<u16> = "PNG, JPEG, WebP\0*.png;*.jpg;*.jpeg;*.webp\0\0"
        .encode_utf16()
        .collect();
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of_val(&ofn) as _;
    ofn.hwndOwner = unsafe { GetActiveWindow() };
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as _;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0)?;
    Some(std::ffi::OsString::from_wide(&buf[..len]).into())
}

pub struct Draft {
    image: RgbaImage,
    thumbnail: Option<RgbaImage>,
    x: f32,
    y: f32,
    pub zoom: f32,
}
impl Draft {
    pub fn open(path: &Path) -> io::Result<Self> {
        let open = || {
            image::ImageReader::open(path)
                .and_then(|r| r.with_guessed_format())
                .map_err(|_| io::Error::other("Could not open the image file."))
        };
        let (w, h) = open()?
            .into_dimensions()
            .map_err(|_| io::Error::other("Unsupported image"))?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 48_000_000 {
            return Err(io::Error::other("Image is too large"));
        }
        let img = open()?
            .decode()
            .map_err(|_| io::Error::other("Unsupported image"))?;
        let image = img.into_rgba8();
        let ratio = 648.0 / w.min(h) as f32;
        let thumbnail = (ratio < 1.0).then(|| {
            imageops::thumbnail(
                &image,
                (w as f32 * ratio).round() as u32,
                (h as f32 * ratio).round() as u32,
            )
        });
        Ok(Self {
            image,
            thumbnail,
            x: w as f32 / 2.,
            y: h as f32 / 2.,
            zoom: 1.,
        })
    }
    fn side(&self) -> f32 {
        self.image.width().min(self.image.height()) as f32 / self.zoom
    }
    pub fn adjust(&mut self, dx: f32, dy: f32, zoom: f32) {
        self.zoom = zoom.clamp(1., 3.);
        let side = self.side();
        let half = side / 2.;
        self.x = (self.x - dx * side / 216.).clamp(half, self.image.width() as f32 - half);
        self.y = (self.y - dy * side / 216.).clamp(half, self.image.height() as f32 - half);
    }
    pub fn preview(&self) -> Image {
        let scale = self.side() / 216.;
        let source = self.thumbnail.as_ref().unwrap_or(&self.image);
        let sx = source.width() as f32 / self.image.width() as f32;
        let sy = source.height() as f32 / self.image.height() as f32;
        surface(&RgbaImage::from_fn(284, 284, |x, y| {
            let (dx, dy) = (x as f32 - 141.5, y as f32 - 141.5);
            let u = (self.x + dx * scale) * sx - 0.5;
            let v = (self.y + dy * scale) * sy - 0.5;
            let mut rgb = imageops::interpolate_bilinear(source, u, v)
                .unwrap_or(image::Rgba([21, 22, 24, 255]));
            let (qx, qy) = (dx.abs() - 60., dy.abs() - 60.);
            let dist = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - 48.;
            if dist > 0. {
                for c in &mut rgb.0[..3] {
                    *c = (*c as f32 * 0.31 + 6.) as u8;
                }
            }
            if dist.abs() < 0.65 {
                rgb = image::Rgba([111, 115, 121, 255]);
            }
            rgb
        }))
    }
    pub fn encode(&self, live: bool) -> io::Result<String> {
        let side = self.side().round().max(1.) as u32;
        let x = ((self.x - self.side() / 2.).round() as u32).min(self.image.width() - side);
        let y = ((self.y - self.side() / 2.).round() as u32).min(self.image.height() - side);
        let square = imageops::crop_imm(&self.image, x, y, side, side).to_image();
        let scaled = imageops::resize(&square, 512, 512, imageops::FilterType::Lanczos3);
        if live {
            return uplink::network_avatar::encode(scaled.as_raw());
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(scaled)
            .write_to(&mut out, ImageFormat::Png)
            .map_err(io::Error::other)?;
        Ok(STANDARD.encode(out.into_inner()))
    }
}

#[cfg(test)]
#[test]
fn invite_qr_decodes_to_the_exact_one_time_link() {
    let url = format!("uplink://lan.example/invite/{}", "01".repeat(32));
    let image = invitation_pixels(&url).unwrap();
    let mut decoder = quircs::Quirc::default();
    let code = decoder
        .identify(
            image.width() as usize,
            image.height() as usize,
            image.as_raw(),
        )
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(code.decode().unwrap().payload, url.as_bytes());
}
