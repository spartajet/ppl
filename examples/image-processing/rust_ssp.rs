/*
MIT License

Copyright (c) 2021 Parallel Applications Modelling Group - GMAP

 GMAP website: https://gmap.pucrs.br
	
 Pontifical Catholic University of Rio Grande do Sul (PUCRS)
 
 Av. Ipiranga, 6681, Porto Alegre - Brazil, 90619-900

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
 */

 use raster::filter;
 use raster::Image;
 use std::time::{SystemTime};
 
 use rust_spp::*;
 
 pub fn rust_ssp(dir_name: &str, threads: usize) {
     let dir_entries = std::fs::read_dir(format!("{}", dir_name));
     let mut all_images: Vec<Image> = Vec::new();
 
     for entry in dir_entries.unwrap() {
         let entry = entry.unwrap();
         let path = entry.path();
 
         if path.extension().is_none() {
             continue;
         }
         all_images.push(raster::open(path.to_str().unwrap()).unwrap());
     }
 
     let start = SystemTime::now();
 
     let pipeline = pipeline![
             parallel!(move |mut image: Image| {
                 filter::saturation(&mut image, 0.2).unwrap();
                 Some(image)
             }, threads as i32),
             parallel!(move |mut image: Image| {
                 filter::emboss(&mut image).unwrap();
                 Some(image)
             }, threads as i32),
             parallel!(move |mut image: Image| {
                 filter::gamma(&mut image, 2.0).unwrap();
                 Some(image)
             }, threads as i32),
             parallel!(move |mut image: Image| {
                 filter::sharpen(&mut image).unwrap();
                 Some(image)
             }, threads as i32),
             parallel!(move |mut image: Image| {
                 filter::grayscale(&mut image).unwrap();
                 Some(image)
             }, threads as i32),
             collect!()
         ];
 
 
     for image in all_images.into_iter() {
         pipeline.post(image).unwrap();
     }
 
     let _collection = pipeline.collect();
 
     let system_duration = start.elapsed().expect("Failed to get render time?");
     let in_sec = system_duration.as_secs() as f64 + system_duration.subsec_nanos() as f64 * 1e-9;
     println!("Execution time: {} sec", in_sec);
 }