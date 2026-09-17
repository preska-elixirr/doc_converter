//! Which output formats each input kind can reach with the engines present.
//! The backend is authoritative; the UI only displays this.

use crate::{office::OfficeEngine, InputKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Pdf,
    Pdfa1b,
    Pdfa2b,
    Pdfa3b,
    Pdfa4,
    Pdfa4f,
    Pdfua1,
    Docx,
    Txt,
    Html,
    Md,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 11] = [
        OutputFormat::Pdf,
        OutputFormat::Pdfa1b,
        OutputFormat::Pdfa2b,
        OutputFormat::Pdfa3b,
        OutputFormat::Pdfa4,
        OutputFormat::Pdfa4f,
        OutputFormat::Pdfua1,
        OutputFormat::Docx,
        OutputFormat::Txt,
        OutputFormat::Html,
        OutputFormat::Md,
    ];
    pub fn ext(self) -> &'static str {
        match self {
            OutputFormat::Pdf
            | OutputFormat::Pdfa1b
            | OutputFormat::Pdfa2b
            | OutputFormat::Pdfa3b
            | OutputFormat::Pdfa4
            | OutputFormat::Pdfa4f
            | OutputFormat::Pdfua1 => "pdf",
            OutputFormat::Docx => "docx",
            OutputFormat::Txt => "txt",
            OutputFormat::Html => "html",
            OutputFormat::Md => "md",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Pdf => "PDF",
            OutputFormat::Pdfa1b => "PDF/A-1b",
            OutputFormat::Pdfa2b => "PDF/A-2b",
            OutputFormat::Pdfa3b => "PDF/A-3b",
            OutputFormat::Pdfa4 => "PDF/A-4",
            OutputFormat::Pdfa4f => "PDF/A-4f",
            OutputFormat::Pdfua1 => "PDF/UA-1",
            OutputFormat::Docx => "DOCX",
            OutputFormat::Txt => "TXT",
            OutputFormat::Html => "HTML",
            OutputFormat::Md => "MD",
        }
    }
    pub fn is_archival(self) -> bool {
        matches!(
            self,
            Self::Pdfa1b | Self::Pdfa2b | Self::Pdfa3b | Self::Pdfa4 | Self::Pdfa4f
        )
    }
    pub fn is_standard_pdf(self) -> bool {
        self.is_archival() || self == Self::Pdfua1
    }
    pub fn supports_attachments(self) -> bool {
        matches!(self, Self::Pdfa3b | Self::Pdfa4f)
    }
    pub fn validation_flavour(self) -> Option<&'static str> {
        match self {
            Self::Pdfa1b => Some("1b"),
            Self::Pdfa2b => Some("2b"),
            Self::Pdfa3b => Some("3b"),
            Self::Pdfa4 => Some("4"),
            Self::Pdfa4f => Some("4f"),
            Self::Pdfua1 => Some("ua1"),
            _ => None,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct Engines {
    pub office: Option<OfficeEngine>,
}

/// How a conversion runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Unavailable(String),
    /// Same format: copy the bytes (DOCX may still get layout edits).
    Copy,
    /// PDF text extraction.
    PdfText,
    /// LibreOffice reads the source and writes the target.
    Office,
    /// Markdown or text paginated by the built-in engine.
    TextToPdf,
    TextToHtml,
    TextToTxt,
    /// HTML to Markdown in-process.
    HtmlToMd,
    /// HTML to Markdown, then the built-in engine (no LibreOffice).
    HtmlToPdfBuiltin,
    HtmlToTxtBuiltin,
    /// LibreOffice writes HTML, then in-process HTML to Markdown.
    OfficeToMdViaHtml,
    /// Built-in Markdown to HTML, then LibreOffice reads the HTML.
    TextToOfficeViaHtml,
}

pub const NO_OFFICE: &str =
    "LibreOffice was not found. Install it or copy it to the engines folder, then restart.";

pub fn route(kind: InputKind, format: OutputFormat, engines: &Engines) -> Route {
    use InputKind as K;
    use OutputFormat as F;
    let office = engines.office.as_ref();
    if format.is_standard_pdf() {
        if !matches!(kind, K::Docx | K::Odt | K::Pptx | K::Xlsx | K::Html) {
            return Route::Unavailable(
                "PDF/A and PDF/UA export support DOCX, ODT, PPTX, XLSX and HTML sources.".into(),
            );
        }
        return match office {
            Some(o) if o.supports_pdfa() => Route::Office,
            _ => Route::Unavailable(
                "PDF/A and PDF/UA export require LibreOffice 25.8 or later with a detected version.".into(),
            ),
        };
    }
    let need_office = |route: Route| match office {
        Some(_) => route,
        None => Route::Unavailable(NO_OFFICE.to_string()),
    };
    match (kind, format) {
        (K::Pdf, F::Pdf) => Route::Copy,
        (K::Pdf, F::Txt) => Route::PdfText,
        (K::Pdf, _) => Route::Unavailable(
            "A PDF keeps its layout. Text can be extracted to TXT; editable formats are not offered."
                .into(),
        ),
        (K::Docx, F::Docx) => Route::Copy,
        (K::Docx | K::Odt, F::Md) => match office {
            Some(o) if o.markdown => Route::Office,
            Some(_) => Route::OfficeToMdViaHtml,
            None => Route::Unavailable(NO_OFFICE.into()),
        },
        (K::Docx | K::Odt, _) => need_office(Route::Office),
        (K::Pptx, F::Pdf) => need_office(Route::Office),
        (K::Pptx, _) => Route::Unavailable("Presentations convert to PDF only.".into()),
        (K::Xlsx, F::Pdf | F::Html) => need_office(Route::Office),
        (K::Xlsx, _) => Route::Unavailable("Spreadsheets convert to PDF or HTML.".into()),
        (K::Md | K::Txt, F::Pdf) => Route::TextToPdf,
        (K::Md | K::Txt, F::Html) => Route::TextToHtml,
        (K::Md, F::Txt) => Route::TextToTxt,
        (K::Txt, F::Txt) | (K::Md, F::Md) | (K::Txt, F::Md) => Route::Copy,
        (K::Md, F::Docx) => match office {
            Some(o) if o.markdown => Route::Office,
            Some(_) => Route::TextToOfficeViaHtml,
            None => Route::Unavailable(NO_OFFICE.into()),
        },
        (K::Txt, F::Docx) => need_office(Route::Office),
        (K::Html, F::Html) => Route::Copy,
        (K::Html, F::Md) => Route::HtmlToMd,
        (K::Html, F::Pdf) => match office {
            Some(_) => Route::Office,
            None => Route::HtmlToPdfBuiltin,
        },
        (K::Html, F::Txt) => match office {
            Some(_) => Route::Office,
            None => Route::HtmlToTxtBuiltin,
        },
        (K::Html, F::Docx) => need_office(Route::Office),
        (_, _) => Route::Unavailable("This file type is not a document.".into()),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Availability {
    pub format: OutputFormat,
    pub available: bool,
    /// `office`, `builtin`, `copy`, or empty when unavailable.
    pub engine: String,
    pub reason: Option<String>,
}

pub fn capabilities(kind: InputKind, engines: &Engines) -> Vec<Availability> {
    OutputFormat::ALL
        .iter()
        .map(|format| {
            let (available, engine, reason) = match route(kind, *format, engines) {
                Route::Unavailable(reason) => (false, "", Some(reason)),
                Route::Copy => (true, "copy", None),
                Route::Office | Route::OfficeToMdViaHtml | Route::TextToOfficeViaHtml => {
                    (true, "office", None)
                }
                _ => (true, "builtin", None),
            };
            Availability {
                format: *format,
                available,
                engine: engine.to_string(),
                reason,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archival_matrix_requires_supported_source_and_known_engine() {
        for format in OutputFormat::ALL
            .into_iter()
            .filter(|f| f.is_standard_pdf())
        {
            assert!(matches!(
                route(InputKind::Docx, format, &Engines::default()),
                Route::Unavailable(_)
            ));
            for (version, supported) in [
                ("LibreOffice", false),
                ("LibreOffice 24.8", false),
                ("LibreOffice 25.2", false),
                ("LibreOffice 25.8", true),
                ("LibreOffice 26.8.0.3", true),
            ] {
                let engines = Engines {
                    office: Some(OfficeEngine {
                        path: "unused".into(),
                        profile: "unused".into(),
                        markdown: true,
                        version: version.into(),
                    }),
                };
                for kind in [
                    InputKind::Docx,
                    InputKind::Odt,
                    InputKind::Pptx,
                    InputKind::Xlsx,
                    InputKind::Html,
                ] {
                    assert_eq!(route(kind, format, &engines) == Route::Office, supported);
                }
                for kind in [
                    InputKind::Pdf,
                    InputKind::Txt,
                    InputKind::Md,
                    InputKind::Png,
                    InputKind::Other,
                ] {
                    assert!(matches!(
                        route(kind, format, &engines),
                        Route::Unavailable(_)
                    ));
                }
            }
        }
    }

    #[test]
    fn matrix_without_office() {
        let engines = Engines::default();
        let md = capabilities(InputKind::Md, &engines);
        assert!(
            md.iter()
                .find(|a| a.format == OutputFormat::Pdf)
                .unwrap()
                .available
        );
        assert!(
            !md.iter()
                .find(|a| a.format == OutputFormat::Docx)
                .unwrap()
                .available
        );
        assert_eq!(
            route(InputKind::Html, OutputFormat::Pdf, &engines),
            Route::HtmlToPdfBuiltin
        );
        assert_eq!(
            route(InputKind::Pdf, OutputFormat::Txt, &engines),
            Route::PdfText
        );
        assert!(matches!(
            route(InputKind::Docx, OutputFormat::Pdf, &engines),
            Route::Unavailable(_)
        ));
        assert!(matches!(
            route(InputKind::Png, OutputFormat::Pdf, &engines),
            Route::Unavailable(_)
        ));
    }
}
