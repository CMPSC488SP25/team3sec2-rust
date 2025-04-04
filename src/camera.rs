use nokhwa::{
    Buffer, Camera,
    pixel_format::RgbFormat,
    utils::{
        ApiBackend, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType, Resolution,
    },
};
use ort::value::Tensor;
use rgb::FromSlice;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::yolo::{self};

use resize::px::RGB;

struct U8ToF32;

impl resize::PixelFormat for U8ToF32 {
    type InputPixel = RGB<u8>;
    type OutputPixel = RGB<f32>;
    type Accumulator = RGB<f32>;

    #[inline(always)]
    fn new() -> Self::Accumulator {
        RGB::new(0., 0., 0.)
    }

    #[inline(always)]
    fn add(&self, acc: &mut Self::Accumulator, inp: RGB<u8>, coeff: f32) {
        acc.r += inp.r as f32 * coeff;
        acc.g += inp.g as f32 * coeff;
        acc.b += inp.b as f32 * coeff;
    }

    #[inline(always)]
    fn add_acc(acc: &mut Self::Accumulator, inp: Self::Accumulator, coeff: f32) {
        acc.r += inp.r * coeff;
        acc.g += inp.g * coeff;
        acc.b += inp.b * coeff;
    }

    #[inline(always)]
    fn into_pixel(&self, acc: Self::Accumulator) -> RGB<f32> {
        RGB {
            r: acc.r / 255.0,
            g: acc.g / 255.0,
            b: acc.b / 255.0,
        }
    }
}

pub async fn cam_plus_yolo_detect() -> Result<(), ()> {
    let mut model = yolo::load_model().expect("The model should load");

    let format = RequestedFormat::with_formats(
        RequestedFormatType::AbsoluteHighestFrameRate,
        &[FrameFormat::MJPEG],
    );

    let mut camera: Camera =
        Camera::with_backend(CameraIndex::Index(0), format, ApiBackend::Video4Linux)
            .expect("Constructing camera should succeed");

    camera
        .set_resolution(Resolution {
            width_x: 1280,
            height_y: 720,
        })
        .expect("setting res should work");

    let res = camera.resolution();
    let width = res.width() as usize;
    let height = res.height() as usize;

    let mut input_img_buffer = vec![0u8; width * height * 3];
    let mut resized_input = Tensor::from_array(([1i64, 3, 640, 640], vec![0_f32; 3 * 640 * 640]))
        .expect("Should construct tensor");

    let mut resizer = resize::new(width, height, 640, 640, U8ToF32, resize::Type::Triangle)
        .expect("resizer should init");

    camera.open_stream().expect("Stream should start");

    // load the yolo model
    // let img_path = "./data/test.jpg"; // change the path if needed
    // let img = ImageReader::open(img_path).unwrap().decode().unwrap();

    // let mut xy = Tensor::from_array(([1i64, 3, 640, 640], vec![0f32; 3 * 640 * 640]))
    //     .expect("Should construct tensor");

    // let img = img.to_rgb32f();

    // let raw = img.clone().into_vec();

    // {
    //     let (_, resized_input_buffer) = xy.extract_raw_tensor_mut();

    //     resizer
    //         .resize(raw.as_rgb(), resized_input_buffer.as_rgb_mut())
    //         .expect("resize should work");
    // }

    // println!("yolo detect test {:?}", yolo::detect(&mut model, xy));

    let (tx, mut rx) = mpsc::channel::<Buffer>(100);

    tokio::spawn(async move {
        let mut frame_count = 0;
        let mut last_time = Instant::now();

        loop {
            let buffer = camera.frame().expect("frame should be retrievable");

            tx.send(buffer).await
                .expect("Should be able to send over channel");
        }
    });

    loop {
        if let Some(buffer) = rx.recv().await {
            buffer
                .decode_image_to_buffer::<RgbFormat>(&mut input_img_buffer)
                .expect("decoding imgae to buffer should work");

            {
                let (_, resized_input_buffer) = resized_input.extract_raw_tensor_mut();

                resizer
                    .resize(input_img_buffer.as_rgb(), resized_input_buffer.as_rgb_mut())
                    .expect("resize should work");
            }

            match yolo::detect(&mut model, resized_input.view()) {
                Ok(_) => (),
                Err(e) => println!("err: {:?}", e),
            }
        }
    }
}
