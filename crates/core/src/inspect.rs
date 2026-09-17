//! Input kind detection. Content is sniffed where a signature exists; plain
//! text formats fall back to the file extension.

use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputKind {
    Pdf,
    Docx,
    Odt,
    Pptx,
    Xlsx,
    Md,
    Html,
    Txt,
    Png,
    Jpg,
    Bmp,
    Webp,
    Tiff,
    Age,
    Other,
}

impl InputKind {
    pub fn is_image(self) -> bool {
        matches!(
            self,
            Self::Png | Self::Jpg | Self::Bmp | Self::Webp | Self::Tiff
        )
    }
    /// Kinds the Convert tab accepts.
    pub fn is_document(self) -> bool {
        matches!(
            self,
            Self::Pdf
                | Self::Docx
                | Self::Odt
                | Self::Pptx
                | Self::Xlsx
                | Self::Md
                | Self::Html
                | Self::Txt
        )
    }
    /// Text sources that the built-in layout engine paginates itself.
    pub fn is_text(self) -> bool {
        matches!(self, Self::Md | Self::Txt)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Docx => "DOCX",
            Self::Odt => "ODT",
            Self::Pptx => "PPTX",
            Self::Xlsx => "XLSX",
            Self::Md => "MD",
            Self::Html => "HTML",
            Self::Txt => "TXT",
            Self::Png => "PNG",
            Self::Jpg => "JPG",
            Self::Bmp => "BMP",
            Self::Webp => "WEBP",
            Self::Tiff => "TIFF",
            Self::Age => "AGE",
            Self::Other => "FILE",
        }
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Detects the kind of `path`. Unreadable files are `Other`.
pub fn inspect(path: &Path) -> InputKind {
    let ext = extension(path);
    let Ok(mut file) = File::open(path) else {
        return InputKind::Other;
    };
    let mut head = [0u8; 64];
    let n = file.read(&mut head).unwrap_or(0);
    let head = &head[..n];
    if head.starts_with(b"%PDF") {
        return InputKind::Pdf;
    }
    if head.starts_with(b"age-encryption.org/v1") {
        return InputKind::Age;
    }
    if head.starts_with(b"PK\x03\x04") {
        if file.seek(std::io::SeekFrom::Start(0)).is_ok() {
            if let Ok(mut zip) = zip::ZipArchive::new(file) {
                if zip.by_name("word/document.xml").is_ok() {
                    return InputKind::Docx;
                }
                if zip.by_name("ppt/presentation.xml").is_ok() {
                    return InputKind::Pptx;
                }
                if zip.by_name("xl/workbook.xml").is_ok() {
                    return InputKind::Xlsx;
                }
                if let Ok(mut mime) = zip.by_name("mimetype") {
                    let mut s = String::new();
                    if mime.read_to_string(&mut s).is_ok()
                        && s.trim() == "application/vnd.oasis.opendocument.text"
                    {
                        return InputKind::Odt;
                    }
                }
            }
        }
        return InputKind::Other;
    }
    match image::guess_format(head) {
        Ok(image::ImageFormat::Png) => return InputKind::Png,
        Ok(image::ImageFormat::Jpeg) => return InputKind::Jpg,
        Ok(image::ImageFormat::Bmp) => return InputKind::Bmp,
        Ok(image::ImageFormat::WebP) => return InputKind::Webp,
        Ok(image::ImageFormat::Tiff) => return InputKind::Tiff,
        _ => {}
    }
    match ext.as_str() {
        "md" | "markdown" => InputKind::Md,
        "html" | "htm" | "xhtml" => InputKind::Html,
        "txt" | "text" => InputKind::Txt,
        _ => InputKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_by_content_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("renamed.txt");
        std::fs::write(&pdf, b"%PDF-1.4 fake").unwrap();
        assert_eq!(inspect(&pdf), InputKind::Pdf);
        let md = dir.path().join("notes.md");
        std::fs::write(&md, "# Title").unwrap();
        assert_eq!(inspect(&md), InputKind::Md);
        let png = dir.path().join("image.dat");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]))
            .save_with_format(&png, image::ImageFormat::Png)
            .unwrap();
        assert_eq!(inspect(&png), InputKind::Png);
        let age = dir.path().join("secret.age");
        std::fs::write(&age, b"age-encryption.org/v1\n").unwrap();
        assert_eq!(inspect(&age), InputKind::Age);
        assert_eq!(inspect(&dir.path().join("missing")), InputKind::Other);
    }
}
