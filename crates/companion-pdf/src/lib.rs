//! GPUI PDF viewer integration and independently owned text snapshot.

pub mod destinations;
pub mod page_index;
pub mod snapshot;
pub mod text_layer;
mod viewer;

pub use viewer::CompanionPdf;
