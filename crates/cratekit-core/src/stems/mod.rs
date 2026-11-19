//! Stem separation (SPEC, Module 4). So far: [`runtime`], which loads ONNX
//! Runtime at run time and says why stems cannot run when it cannot. The
//! separator itself comes with #31. See docs/stems.md and
//! docs/ARCHITECTURE.md, Stems notes.

pub mod runtime;
