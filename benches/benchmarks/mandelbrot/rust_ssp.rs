use image::{ImageBuffer, Luma};
use num_complex::Complex;

use rust_spp::*;

pub fn rust_ssp(threads: usize) {
    let max_iterations = 10000u16;
    let img_side = 1000u32;
    let cxmin = -2f32;
    let cxmax = 1f32;
    let cymin = -1.5f32;
    let cymax = 1.5f32;
    let scalex = (cxmax - cxmin) / img_side as f32;
    let scaley = (cymax - cymin) / img_side as f32;

    // Create the lines
    let lines: Vec<u32> = (0..img_side).collect();

    let pipeline = pipeline![
        parallel!(
            move |y| {
                let mut row = Vec::with_capacity(img_side as usize);
                for x in 0..img_side {
                    let cx = cxmin + x as f32 * scalex;
                    let cy = cymin + y as f32 * scaley;

                    let c = Complex::new(cx, cy);
                    let mut z = Complex::new(0f32, 0f32);

                    let mut i = 0;
                    for t in 0..max_iterations {
                        if z.norm() > 2.0 {
                            break;
                        }
                        z = z * z + c;
                        i = t;
                    }

                    row.push(image::Luma([i as u8]));
                }
                Some(row)
            },
            threads as i32
        ),
        collect_ordered!()
    ];

    for coord in lines.into_iter() {
        pipeline.post(coord).unwrap();
    }

    let mut res: Vec<Luma<u8>> = pipeline
        .collect()
        .into_iter()
        .flat_map(|a| a.to_vec())
        .collect();

    let mut imgbuf: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::new(img_side, img_side);
    for (_, _, pixel) in imgbuf.enumerate_pixels_mut() {
        *pixel = res.remove(0);
    }
    imgbuf
        .save("benches/benchmarks/mandelbrot/fractal_rust_ssp.png")
        .unwrap();
}
