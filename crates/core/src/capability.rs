//! Which output formats each input kind can reach with the engines present.
//! The backend is authoritative; the UI only displays this.

use crate::{office::OfficeEngine, InputKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Pdf,
    Docx,
    Txt,
    Html,
    Md,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 5] = [
        OutputFormat::Pdf,
        OutputFormat::Docx,
        OutputFormat::Txt,
        OutputFormat::Html,
        OutputFormat::Md,
    ];
    pub fn ext(self) -> &'static str {
        match self {
            OutputFormat::Pdf => "pdf",
            OutputFormat::Docx => "docx",
            OutputFormat::Txt => "txt",
            OutputFormat::Html => "html",
            OutputFormat::Md => "md",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            OutputFormat::Pdf => "PDF",
            OutputFormat::Docx => "DOCX",
            OutputFormat::Txt => "TXT",
            OutputFormat::Html => "HTML",
            OutputFormat::Md => "MD",
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
