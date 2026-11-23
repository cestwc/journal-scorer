use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfError {
    #[error("PDF engine error: {0}")]
    Engine(String),
    #[error("the PDF has no extractable text or figures")]
    Empty,
}

impl From<mupdf::Error> for PdfError {
    fn from(e: mupdf::Error) -> Self {
        PdfError::Engine(e.to_string())
    }
}
