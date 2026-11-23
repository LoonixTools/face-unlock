//! Camera, finding and recognising faces, telling a face from a photo of
//! one, and the face store. All of it in Rust: the networks run on tract, and
//! the bits of image processing the checks need are in img.

pub mod camera;
pub mod face;
pub mod img;
pub mod liveness;
pub mod store;

#[cfg(test)]
mod tests;
