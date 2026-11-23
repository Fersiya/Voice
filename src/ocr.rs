use anyhow::Result;
use image::RgbaImage;

pub struct Ocr {
    #[cfg(windows)]
    engine: windows::Media::Ocr::OcrEngine,
}

#[cfg(windows)]
impl Ocr {
    pub fn new(lang: &str) -> Result<Self> {
        use anyhow::anyhow;
        use windows::{
            core::HSTRING,
            Globalization::Language,
            Media::Ocr::OcrEngine,
            Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
        };

        unsafe {
            // Повторная инициализация вернёт S_FALSE — это нормально.
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let try_lang = |tag: &str| -> Option<OcrEngine> {
            let language = Language::CreateLanguage(&HSTRING::from(tag)).ok()?;
            OcrEngine::TryCreateFromLanguage(&language).ok()
        };
        // «ru» → «ru-RU», затем языки профиля пользователя.
        let engine = try_lang(lang)
            .or_else(|| (!lang.contains('-')).then(|| try_lang(&format!("{lang}-{}", lang.to_uppercase()))).flatten())
            .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
            .ok_or_else(|| {
                anyhow!(
                    "OCR для «{lang}» недоступен: в Windows не установлен компонент распознавания текста. \
                     Открой PowerShell от администратора и выполни: \
                     Add-WindowsCapability -Online -Name \"Language.OCR~~~ru-RU~0.0.1.0\" — потом перезапусти Voice"
                )
            })?;
        Ok(Self { engine })
    }

    pub fn read(&self, img: &RgbaImage) -> Result<String> {
        use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
        use windows::{
            Graphics::Imaging::BitmapDecoder,
            Storage::Streams::{DataWriter, InMemoryRandomAccessStream},
        };

        // Мелкий текст Windows OCR читает заметно лучше после увеличения.
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

#[cfg(not(windows))]
impl Ocr {
    pub fn new(_lang: &str) -> Result<Self> {
        Err(anyhow::anyhow!("OCR работает только в Windows"))
    }

    pub fn read(&self, _img: &RgbaImage) -> Result<String> {
        Ok(String::new())
    }
}
