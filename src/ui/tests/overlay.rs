use super::*;

/// One channel, straight over black.
struct Gray {
    width: usize,
    pixels: Vec<f32>,
}

impl Target for Gray {
    fn size(&self) -> (usize, usize) {
        (self.width, self.pixels.len() / self.width)
    }

    fn signal(&self, srgb: [f32; 3]) -> [f32; 3] {
        srgb
    }

    fn blend(&mut self, x: usize, y: usize, signal: [f32; 3], alpha: f32) {
        let pixel = &mut self.pixels[y * self.width + x];
        *pixel = signal[0] * alpha + *pixel * (1.0 - alpha);
    }
}

#[test]
fn samples_bitmaps_the_way_they_are_turned() {
    // A 2 x 1 image, white on the left and clear on the right, turned a
    // quarter clockwise into a 1 x 2 spot: white goes to the top.
    let pixels = [255, 255, 255, 255, 0, 0, 0, 0];
    let image = RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::from_raw(2, 1, pixels.to_vec()).unwrap(),
    )]);
    let turned = ContentTransform {
        origin: [0.0, 1.0],
        across: [0.0, -1.0],
        down: [1.0, 0.0],
    };
    let mut target = Gray {
        width: 1,
        pixels: vec![0.0; 2],
    };
    draw_image(&mut target, &image, [0.0, 0.0, 1.0, 1.0], turned);
    assert_eq!(target.pixels, [1.0, 0.0]);
}
