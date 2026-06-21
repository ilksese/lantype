use base64::Engine;
use image::Luma;
use qrcode::QrCode;

pub fn generate_qr_png(data: &str) -> Result<Vec<u8>, String> {
    let code = QrCode::new(data).map_err(|e| format!("qr encode: {e}"))?;
    let image = code
        .render::<Luma<u8>>()
        .min_dimensions(256, 256)
        .dark_color(Luma([0]))
        .light_color(Luma([255]))
        .build();

    let mut buf = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("png encode: {e}"))?;
    Ok(buf)
}

pub fn qr_data_url(data: &str) -> Result<String, String> {
    let png = generate_qr_png(data)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{b64}"))
}