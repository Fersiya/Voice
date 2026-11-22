use anyhow::{anyhow, Result};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder, RgbaImage};
use windows::{
    Globalization::Language,
    Graphics::Imaging::BitmapDecoder,
    Media::Ocr::OcrEngine,
    Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
    Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
};
use windows::core::HSTRING;

pub struct Ocr {
    engine: OcrEngine,
}

impl Ocr {
    pub fn new(lang: &str) -> Result<Self> {
        unsafe {
            // Повторный вызов вернёт S_FALSE — это нормально.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let language = Language::CreateLanguage(&HSTRING::from(lang))?;
        let engine = OcrEngine::TryCreateFromLanguage(&language).map_err(|_| {
            anyhow!("OCR для языка «{lang}» недоступен: установи языковой пакет в Параметры → Время и язык")
        })?;
        Ok(Self { engine })
    }

    pub fn read(&self, img: &RgbaImage) -> Result<String> {
        // Windows OCR лучше работает на увеличенной картинке мелкого текста.
        let scaled = image::imageops::resize(
            img,
            img.width() * 2,
            img.height() * 2,
            image::imageops::FilterType::CatmullRom,
        );

        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(
            scaled.as_raw(),
            scaled.width(),
            scaled.height(),
            ExtendedColorType::Rgba8,
        )?;

        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(&png)?;
        writer.StoreAsync()?.get()?;
        writer.FlushAsync()?.get()?;
        writer.DetachStream()?;
        stream.Seek(0)?;

        let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;
        let result = self.engine.RecognizeAsync(&bitmap)?.get()?;
        Ok(result.Text()?.to_string())
    }
}
