use pspp::core::orchestrator::Orchestrator;
use pspp::thread_pool::ThreadPool;
use raster::filter;
use raster::Image;

pub fn pspp_map(images: Vec<Image>, threads: usize) {
    let mut pool = ThreadPool::new_with_global_registry(threads * 5);
    /*
        let a = pool.par_map(images, |mut image: Image| -> Image {
        filter::saturation(&mut image, 0.2).unwrap();
        image
    });
    let b = pool.par_map(a, |mut image: Image| -> Image {
        filter::emboss(&mut image).unwrap();
        image
    });
    let c = pool.par_map(b, |mut image: Image| -> Image {
        filter::gamma(&mut image, 2.0).unwrap();
        image
    });
    let d = pool.par_map(c, |mut image: Image| {
        filter::sharpen(&mut image).unwrap();
        image
    });
    let _e: Vec<Image> = pool.par_map(d, |mut image: Image| {
        filter::grayscale(&mut image).unwrap();
        image
    })
    .collect();
    */

    let _res: Vec<Image> = pool
        .par_map(images, |mut image: Image| {
            filter::saturation(&mut image, 0.2).unwrap();
            filter::emboss(&mut image).unwrap();
            filter::gamma(&mut image, 2.0).unwrap();
            filter::sharpen(&mut image).unwrap();
            filter::grayscale(&mut image).unwrap();
            image
        })
        .collect();

    Orchestrator::delete_global_orchestrator();
}
