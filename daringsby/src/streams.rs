#[cfg(feature = "canvas-stream")]
pub use crate::canvas_stream::CanvasStream;
pub use crate::speech_stream::SpeechStream;
#[cfg(feature = "svg-motor")]
pub use crate::svg_motor::SvgMotor;
/// Stream sources provided by the Daringsby runtime.
///
/// # Examples
/// ```
/// use daringsby::streams::SpeechStream;
/// ```
pub use crate::vision_sensor::VisionSensor;
