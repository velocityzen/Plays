/* Functions useful for debugging:

// A heat map color for debugging (blue -> cyan -> green -> yellow -> red).
fn heat_map_color(value: f32, minValue: f32, maxValue: f32, position: vec2<f32>) -> vec4<f32> {
    // Normalize value to 0-1 range
    let t = clamp((value - minValue) / (maxValue - minValue), 0.0, 1.0);

    // Heat map color calculation
    let r = t * t;
    let g = 4.0 * t * (1.0 - t);
    let b = (1.0 - t) * (1.0 - t);
    let heat_color = vec3<f32>(r, g, b);

    // Create a checkerboard pattern (black and white)
    let sum = floor(position.x / 3) + floor(position.y / 3);
    let is_odd = fract(sum * 0.5); // 0.0 for even, 0.5 for odd
    let checker_value = is_odd * 2.0; // 0.0 for even, 1.0 for odd
    let checker_color = vec3<f32>(checker_value);

    // Determine if value is in range (1.0 if in range, 0.0 if out of range)
    let in_range = step(minValue, value) * step(value, maxValue);

    // Mix checkerboard and heat map based on whether value is in range
    let final_color = mix(checker_color, heat_color, in_range);

    return vec4<f32>(final_color, 1.0);
}

*/

fn color_brightness(color: vec3<f32>) -> f32 {
    // REC. 601 luminance coefficients for perceived brightness
    return dot(color, vec3<f32>(0.30, 0.59, 0.11));
}

fn light_on_dark_contrast(enhancedContrast: f32, color: vec3<f32>) -> f32 {
    let brightness = color_brightness(color);
    let multiplier = saturate(4.0 * (0.75 - brightness));
    return enhancedContrast * multiplier;
}

fn enhance_contrast(alpha: f32, k: f32) -> f32 {
    return alpha * (k + 1.0) / (alpha * k + 1.0);
}

fn apply_alpha_correction(a: f32, b: f32, g: vec4<f32>) -> f32 {
    let brightness_adjustment = g.x * b + g.y;
    let correction = brightness_adjustment * a + (g.z * b + g.w);
    return a + a * (1.0 - a) * correction;
}

fn apply_contrast_and_gamma_correction(sample: f32, color: vec3<f32>, enhanced_contrast_factor: f32, gamma_ratios: vec4<f32>) -> f32 {
    let enhanced_contrast = light_on_dark_contrast(enhanced_contrast_factor, color);
    let brightness = color_brightness(color);

    let contrasted = enhance_contrast(sample, enhanced_contrast);
    return apply_alpha_correction(contrasted, brightness, gamma_ratios);
}

struct GlobalParams {
    viewport_size: vec2<f32>,
    premultiplied_alpha: u32,
    pad: u32,
}

var<uniform> globals: GlobalParams;
var<uniform> gamma_ratios: vec4<f32>;
var<uniform> grayscale_enhanced_contrast: f32;
var t_sprite: texture_2d<f32>;
var s_sprite: sampler;

const M_PI_F: f32 = 3.1415926;
const GRAYSCALE_FACTORS: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

struct Bounds {
    origin: vec2<f32>,
    size: vec2<f32>,
}

struct Corners {
    top_left: f32,
    top_right: f32,
    bottom_right: f32,
    bottom_left: f32,
}

struct Edges {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

struct Hsla {
    h: f32,
    s: f32,
    l: f32,
    a: f32,
}

struct LinearColorStop {
    color: Hsla,
    percentage: f32,
}

struct Background {
    // 0u is Solid
    // 1u is LinearGradient
    // 2u is PatternSlash
    tag: u32,
    // 0u is sRGB linear color
    // 1u is Oklab color
    color_space: u32,
    solid: Hsla,
    gradient_angle_or_pattern_height: f32,
    colors: array<LinearColorStop, 2>,
    pad: u32,
}

struct AtlasTextureId {
    index: u32,
    kind: u32,
}

struct AtlasBounds {
    origin: vec2<i32>,
    size: vec2<i32>,
}

struct AtlasTile {
    texture_id: AtlasTextureId,
    tile_id: u32,
    padding: u32,
    bounds: AtlasBounds,
}

struct TransformationMatrix {
    rotation_scale: mat2x2<f32>,
    translation: vec2<f32>,
}

fn to_device_position_impl(position: vec2<f32>) -> vec4<f32> {
    let device_position = position / globals.viewport_size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0);
    return vec4<f32>(device_position, 0.0, 1.0);
}

fn to_device_position(unit_vertex: vec2<f32>, bounds: Bounds) -> vec4<f32> {
    let position = unit_vertex * vec2<f32>(bounds.size) + bounds.origin;
    return to_device_position_impl(position);
}

fn to_device_position_transformed(unit_vertex: vec2<f32>, bounds: Bounds, transform: TransformationMatrix) -> vec4<f32> {
    let position = unit_vertex * vec2<f32>(bounds.size) + bounds.origin;
    //Note: Rust side stores it as row-major, so transposing here
    let transformed = transpose(transform.rotation_scale) * position + transform.translation;
    return to_device_position_impl(transformed);
}

fn to_tile_position(unit_vertex: vec2<f32>, tile: AtlasTile) -> vec2<f32> {
  let atlas_size = vec2<f32>(textureDimensions(t_sprite, 0));
  return (vec2<f32>(tile.bounds.origin) + unit_vertex * vec2<f32>(tile.bounds.size)) / atlas_size;
}

fn distance_from_clip_rect_impl(position: vec2<f32>, clip_bounds: Bounds) -> vec4<f32> {
    let tl = position - clip_bounds.origin;
    let br = clip_bounds.origin + clip_bounds.size - position;
    return vec4<f32>(tl.x, br.x, tl.y, br.y);
}

fn distance_from_clip_rect(unit_vertex: vec2<f32>, bounds: Bounds, clip_bounds: Bounds) -> vec4<f32> {
    let position = unit_vertex * vec2<f32>(bounds.size) + bounds.origin;
    return distance_from_clip_rect_impl(position, clip_bounds);
}

fn distance_from_clip_rect_transformed(unit_vertex: vec2<f32>, bounds: Bounds, clip_bounds: Bounds, transform: TransformationMatrix) -> vec4<f32> {
    let position = unit_vertex * vec2<f32>(bounds.size) + bounds.origin;
    let transformed = transpose(transform.rotation_scale) * position + transform.translation;
    return distance_from_clip_rect_impl(transformed, clip_bounds);
}

// https://gamedev.stackexchange.com/questions/92015/optimized-linear-to-srgb-glsl
fn srgb_to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    let lower = srgb / vec3<f32>(12.92);
    return select(higher, lower, cutoff);
}

fn srgb_to_linear_component(a: f32) -> f32 {
    let cutoff = a < 0.04045;
    let higher = pow((a + 0.055) / 1.055, 2.4);
    let lower = a / 12.92;
    return select(higher, lower, cutoff);
}

fn linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let cutoff = linear < vec3<f32>(0.0031308);
    let higher = vec3<f32>(1.055) * pow(linear, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    let lower = linear * vec3<f32>(12.92);
    return select(higher, lower, cutoff);
}

/// Convert a linear color to sRGBA space.
fn linear_to_srgba(color: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(linear_to_srgb(color.rgb), color.a);
}

/// Convert a sRGBA color to linear space.
fn srgba_to_linear(color: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(srgb_to_linear(color.rgb), color.a);
}

/// Hsla to linear RGBA conversion.
fn hsla_to_rgba(hsla: Hsla) -> vec4<f32> {
    let h = hsla.h * 6.0; // Now, it's an angle but scaled in [0, 6) range
    let s = hsla.s;
    let l = hsla.l;
    let a = hsla.a;

    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - abs(h % 2.0 - 1.0));
    let m = l - c / 2.0;
    var color = vec3<f32>(m);

    if (h >= 0.0 && h < 1.0) {
        color.r += c;
        color.g += x;
    } else if (h >= 1.0 && h < 2.0) {
        color.r += x;
        color.g += c;
    } else if (h >= 2.0 && h < 3.0) {
        color.g += c;
        color.b += x;
    } else if (h >= 3.0 && h < 4.0) {
        color.g += x;
        color.b += c;
    } else if (h >= 4.0 && h < 5.0) {
        color.r += x;
        color.b += c;
    } else {
        color.r += c;
        color.b += x;
    }

    return vec4<f32>(color, a);
}

/// Convert a linear sRGB to Oklab space.
/// Reference: https://bottosson.github.io/posts/oklab/#converting-from-linear-srgb-to-oklab
fn linear_srgb_to_oklab(color: vec4<f32>) -> vec4<f32> {
	let l = 0.4122214708 * color.r + 0.5363325363 * color.g + 0.0514459929 * color.b;
	let m = 0.2119034982 * color.r + 0.6806995451 * color.g + 0.1073969566 * color.b;
	let s = 0.0883024619 * color.r + 0.2817188376 * color.g + 0.6299787005 * color.b;

	let l_ = pow(l, 1.0 / 3.0);
	let m_ = pow(m, 1.0 / 3.0);
	let s_ = pow(s, 1.0 / 3.0);

	return vec4<f32>(
		0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
		1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
		0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
		color.a
	);
}

/// Convert an Oklab color to linear sRGB space.
fn oklab_to_linear_srgb(color: vec4<f32>) -> vec4<f32> {
	let l_ = color.r + 0.3963377774 * color.g + 0.2158037573 * color.b;
	let m_ = color.r - 0.1055613458 * color.g - 0.0638541728 * color.b;
	let s_ = color.r - 0.0894841775 * color.g - 1.2914855480 * color.b;

	let l = l_ * l_ * l_;
	let m = m_ * m_ * m_;
	let s = s_ * s_ * s_;

	return vec4<f32>(
		4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
		-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
		-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
		color.a
	);
}

fn over(below: vec4<f32>, above: vec4<f32>) -> vec4<f32> {
    let alpha = above.a + below.a * (1.0 - above.a);
    // Plays: two clear colors blend to clear, not 0 / 0.
    if (alpha <= 0.0) {
        return vec4<f32>(0.0);
    }
    let color = (above.rgb * above.a + below.rgb * below.a * (1.0 - above.a)) / alpha;
    return vec4<f32>(color, alpha);
}

// A standard gaussian function, used for weighting samples
fn gaussian(x: f32, sigma: f32) -> f32{
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * M_PI_F) * sigma);
}

// This approximates the error function, needed for the gaussian integral
fn erf(v: vec2<f32>) -> vec2<f32> {
    let s = sign(v);
    let a = abs(v);
    let r1 = 1.0 + (0.278393 + (0.230389 + (0.000972 + 0.078108 * a) * a) * a) * a;
    let r2 = r1 * r1;
    return s - s / (r2 * r2);
}

fn blur_along_x(x: f32, y: f32, sigma: f32, corner: f32, half_size: vec2<f32>) -> f32 {
  let delta = min(half_size.y - corner - abs(y), 0.0);
  let curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
  let integral = 0.5 + 0.5 * erf((x + vec2<f32>(-curved, curved)) * (sqrt(0.5) / sigma));
  return integral.y - integral.x;
}

// Selects corner radius based on quadrant.
fn pick_corner_radius(center_to_point: vec2<f32>, radii: Corners) -> f32 {
    if (center_to_point.x < 0.0) {
        if (center_to_point.y < 0.0) {
            return radii.top_left;
        } else {
            return radii.bottom_left;
        }
    } else {
        if (center_to_point.y < 0.0) {
            return radii.top_right;
        } else {
            return radii.bottom_right;
        }
    }
}

// Signed distance of the point to the quad's border - positive outside the
// border, and negative inside.
//
// See comments on similar code using `quad_sdf_impl` in `fs_quad` for
// explanation.
fn quad_sdf(point: vec2<f32>, bounds: Bounds, corner_radii: Corners) -> f32 {
    let half_size = bounds.size / 2.0;
    let center = bounds.origin + half_size;
    let center_to_point = point - center;
    let corner_radius = pick_corner_radius(center_to_point, corner_radii);
    let corner_to_point = abs(center_to_point) - half_size;
    let corner_center_to_point = corner_to_point + corner_radius;
    return quad_sdf_impl(corner_center_to_point, corner_radius);
}

fn quad_sdf_impl(corner_center_to_point: vec2<f32>, corner_radius: f32) -> f32 {
    if (corner_radius == 0.0) {
        // Fast path for unrounded corners.
        return max(corner_center_to_point.x, corner_center_to_point.y);
    } else {
        // Signed distance of the point from a quad that is inset by corner_radius.
        // It is negative inside this quad, and positive outside.
        let signed_distance_to_inset_quad =
            // 0 inside the inset quad, and positive outside.
            length(max(vec2<f32>(0.0), corner_center_to_point)) +
            // 0 outside the inset quad, and negative inside.
            min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));

        return signed_distance_to_inset_quad - corner_radius;
    }
}

// Plays: the extent and exponent of a corner of `corner_radius` with
// `smoothing` (see `Quad::corner_smoothing`). A smoothed corner is a
// superellipse, |x|^n + |y|^n = extent^n from the corner's center: it curves
// over (1 + e) times the radius with exponent 2 + e * 10/3, where e is the
// smoothing, or less when the quad has no room for it. Its curvature is zero
// where it meets the straight sides, so there is no kink there. 0.6 makes
// exponent 4 over 1.6 times the radius, close to Apple's continuous corners.
// A corner as round as the quad allows, or without smoothing, stays a
// circular arc (exponent 2), drawn exactly as before.
fn squircle_corner(corner_radius: f32, half_size: vec2<f32>, smoothing: f32) -> vec2<f32> {
    let room = min(half_size.x, half_size.y);
    if (smoothing <= 0.0 || corner_radius <= 0.0 || room <= 0.0) {
        return vec2<f32>(corner_radius, 2.0);
    }
    let radius = min(corner_radius, room);
    let e = clamp(room / radius - 1.0, 0.0, smoothing);
    return vec2<f32>(radius * (1.0 + e), 2.0 + e * (10.0 / 3.0));
}

// Plays: `quad_sdf`, with the corners as far superellipses as `smoothing`
// says (see `squircle_corner`); without smoothing it is `quad_sdf`.
fn quad_sdf_smoothed(point: vec2<f32>, bounds: Bounds, corner_radii: Corners, smoothing: f32) -> f32 {
    let half_size = bounds.size / 2.0;
    let center = bounds.origin + half_size;
    let center_to_point = point - center;
    let corner = squircle_corner(pick_corner_radius(center_to_point, corner_radii),
                                 half_size, smoothing);
    let corner_center_to_point = abs(center_to_point) - half_size + corner.x;
    if (corner.y == 2.0) {
        return quad_sdf_impl(corner_center_to_point, corner.x);
    }
    return squircle_sdf(corner_center_to_point, corner.x, corner.y);
}

// Plays: signed distance of a point from a superellipse corner, positive
// outside. `corner_center_to_point` is from the corner's center, both
// components > 0, and `radii` its extent along x and y.
fn squircle_corner_sdf(corner_center_to_point: vec2<f32>, radii: vec2<f32>, exponent: f32) -> f32 {
    let r = max(radii, vec2<f32>(0.001));
    let a = corner_center_to_point / r;
    let norm = pow(pow(a.x, exponent) + pow(a.y, exponent), 1.0 / exponent);
    if (norm < 0.001) {
        return -min(r.x, r.y);
    }
    // Divided by the length of the gradient, so near the edge it is a
    // distance in pixels and antialiasing and borders are even all round.
    let gradient = pow(a / norm, vec2<f32>(exponent - 1.0)) / r;
    return (norm - 1.0) / max(length(gradient), 0.0001);
}

// Plays: `quad_sdf_impl` for a superellipse corner.
fn squircle_sdf(corner_center_to_point: vec2<f32>, extent: f32, exponent: f32) -> f32 {
    if (corner_center_to_point.x > 0.0 && corner_center_to_point.y > 0.0) {
        return squircle_corner_sdf(corner_center_to_point, vec2<f32>(extent), exponent);
    }
    return max(corner_center_to_point.x, corner_center_to_point.y) - extent;
}

// Abstract away the final color transformation based on the
// target alpha compositing mode.
fn blend_color(color: vec4<f32>, alpha_factor: f32) -> vec4<f32> {
    let alpha = color.a * alpha_factor;
    let multiplier = select(1.0, alpha, globals.premultiplied_alpha != 0u);
    return vec4<f32>(color.rgb * multiplier, alpha);
}


struct GradientColor {
    solid: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
}

fn prepare_gradient_color(tag: u32, color_space: u32,
    solid: Hsla, colors: array<LinearColorStop, 2>) -> GradientColor {
    var result = GradientColor();

    if (tag == 0u || tag == 2u) {
        result.solid = hsla_to_rgba(solid);
    } else if (tag == 1u) {
        // The hsla_to_rgba is returns a linear sRGB color
        result.color0 = hsla_to_rgba(colors[0].color);
        result.color1 = hsla_to_rgba(colors[1].color);

        // Prepare color space in vertex for avoid conversion
        // in fragment shader for performance reasons
        if (color_space == 0u) {
            // sRGB
            result.color0 = linear_to_srgba(result.color0);
            result.color1 = linear_to_srgba(result.color1);
        } else if (color_space == 1u) {
            // Oklab
            result.color0 = linear_srgb_to_oklab(result.color0);
            result.color1 = linear_srgb_to_oklab(result.color1);
        }
    }

    return result;
}

fn gradient_color(background: Background, position: vec2<f32>, bounds: Bounds,
    solid_color: vec4<f32>, color0: vec4<f32>, color1: vec4<f32>) -> vec4<f32> {
    var background_color = vec4<f32>(0.0);

    switch (background.tag) {
        default: {
            return solid_color;
        }
        case 1u: {
            // Linear gradient background.
            // -90 degrees to match the CSS gradient angle.
            let angle = background.gradient_angle_or_pattern_height;
            let radians = (angle % 360.0 - 90.0) * M_PI_F / 180.0;
            var direction = vec2<f32>(cos(radians), sin(radians));
            let stop0_percentage = background.colors[0].percentage;
            let stop1_percentage = background.colors[1].percentage;

            // Expand the short side to be the same as the long side
            if (bounds.size.x > bounds.size.y) {
                direction.y *= bounds.size.y / bounds.size.x;
            } else {
                direction.x *= bounds.size.x / bounds.size.y;
            }

            // Get the t value for the linear gradient with the color stop percentages.
            let half_size = bounds.size / 2.0;
            let center = bounds.origin + half_size;
            let center_to_point = position - center;
            var t = dot(center_to_point, direction) / length(direction);
            // Check the direct to determine the use x or y
            if (abs(direction.x) > abs(direction.y)) {
                t = (t + half_size.x) / bounds.size.x;
            } else {
                t = (t + half_size.y) / bounds.size.y;
            }

            // Adjust t based on the stop percentages
            t = (t - stop0_percentage) / (stop1_percentage - stop0_percentage);
            t = clamp(t, 0.0, 1.0);

            switch (background.color_space) {
                default: {
                    background_color = srgba_to_linear(mix(color0, color1, t));
                }
                case 1u: {
                    let oklab_color = mix(color0, color1, t);
                    background_color = oklab_to_linear_srgb(oklab_color);
                }
            }
        }
        case 2u: {
            let gradient_angle_or_pattern_height = background.gradient_angle_or_pattern_height;
            let pattern_width = (gradient_angle_or_pattern_height / 65535.0f) / 255.0f;
            let pattern_interval = (gradient_angle_or_pattern_height % 65535.0f) / 255.0f;
            let pattern_height = pattern_width + pattern_interval;
            let stripe_angle = M_PI_F / 4.0;
            let pattern_period = pattern_height * sin(stripe_angle);
            let rotation = mat2x2<f32>(
                cos(stripe_angle), -sin(stripe_angle),
                sin(stripe_angle), cos(stripe_angle)
            );
            let relative_position = position - bounds.origin;
            let rotated_point = rotation * relative_position;
            let pattern = rotated_point.x % pattern_period;
            let distance = min(pattern, pattern_period - pattern) - pattern_period * (pattern_width / pattern_height) /  2.0f;
            background_color = solid_color;
            background_color.a *= saturate(0.5 - distance);
        }
    }

    return background_color;
}

// --- quads --- //

struct Quad {
    order: u32,
    border_style: u32,
    bounds: Bounds,
    content_mask: Bounds,
    background: Background,
    border_color: Hsla,
    corner_radii: Corners,
    border_widths: Edges,
    // Plays: see `Quad::corner_smoothing`.
    corner_smoothing: f32,
    pad: u32,
}
var<storage, read> b_quads: array<Quad>;

struct QuadVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) border_color: vec4<f32>,
    @location(1) @interpolate(flat) quad_id: u32,
    // TODO: use `clip_distance` once Naga supports it
    @location(2) clip_distances: vec4<f32>,
    @location(3) @interpolate(flat) background_solid: vec4<f32>,
    @location(4) @interpolate(flat) background_color0: vec4<f32>,
    @location(5) @interpolate(flat) background_color1: vec4<f32>,
}

@vertex
fn vs_quad(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> QuadVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let quad = b_quads[instance_id];

    var out = QuadVarying();
    out.position = to_device_position(unit_vertex, quad.bounds);

    let gradient = prepare_gradient_color(
        quad.background.tag,
        quad.background.color_space,
        quad.background.solid,
        quad.background.colors
    );
    out.background_solid = gradient.solid;
    out.background_color0 = gradient.color0;
    out.background_color1 = gradient.color1;
    out.border_color = hsla_to_rgba(quad.border_color);
    out.quad_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, quad.bounds, quad.content_mask);
    return out;
}

@fragment
fn fs_quad(input: QuadVarying) -> @location(0) vec4<f32> {
    // Alpha clip first, since we don't have `clip_distance`.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let quad = b_quads[input.quad_id];

    let background_color = gradient_color(quad.background, input.position.xy, quad.bounds,
        input.background_solid, input.background_color0, input.background_color1);

    let unrounded = quad.corner_radii.top_left == 0.0 &&
        quad.corner_radii.bottom_left == 0.0 &&
        quad.corner_radii.top_right == 0.0 &&
        quad.corner_radii.bottom_right == 0.0;

    // Fast path when the quad is not rounded and doesn't have any border
    if (quad.border_widths.top == 0.0 &&
            quad.border_widths.left == 0.0 &&
            quad.border_widths.right == 0.0 &&
            quad.border_widths.bottom == 0.0 &&
            unrounded) {
        return blend_color(background_color, 1.0);
    }

    let size = quad.bounds.size;
    let half_size = size / 2.0;
    let point = input.position.xy - quad.bounds.origin;
    let center_to_point = point - half_size;

    // Signed distance field threshold for inclusion of pixels. 0.5 is the
    // minimum distance between the center of the pixel and the edge.
    let antialias_threshold = 0.5;

    // Radius of the nearest corner. Plays: for a superellipse corner, how far
    // along the sides it curves, and its exponent (2 for a circular arc).
    let corner = squircle_corner(
        pick_corner_radius(center_to_point, quad.corner_radii), half_size,
        quad.corner_smoothing);
    let corner_radius = corner.x;
    let corner_exponent = corner.y;

    // Width of the nearest borders
    let border = vec2<f32>(
        select(
            quad.border_widths.right,
            quad.border_widths.left,
            center_to_point.x < 0.0),
        select(
            quad.border_widths.bottom,
            quad.border_widths.top,
            center_to_point.y < 0.0));

    // 0-width borders are reduced so that `inner_sdf >= antialias_threshold`.
    // The purpose of this is to not draw antialiasing pixels in this case.
    let reduced_border =
        vec2<f32>(select(border.x, -antialias_threshold, border.x == 0.0),
                  select(border.y, -antialias_threshold, border.y == 0.0));

    // Vector from the corner of the quad bounds to the point, after mirroring
    // the point into the bottom right quadrant. Both components are <= 0.
    let corner_to_point = abs(center_to_point) - half_size;

    // Vector from the point to the center of the rounded corner's circle, also
    // mirrored into bottom right quadrant.
    let corner_center_to_point = corner_to_point + corner_radius;

    // Whether the nearest point on the border is rounded
    let is_near_rounded_corner =
            corner_center_to_point.x >= 0 &&
            corner_center_to_point.y >= 0;

    // Vector from straight border inner corner to point.
    let straight_border_inner_corner_to_point = corner_to_point + reduced_border;

    // Whether the point is beyond the inner edge of the straight border.
    let is_beyond_inner_straight_border =
            straight_border_inner_corner_to_point.x > 0 ||
            straight_border_inner_corner_to_point.y > 0;

    // Whether the point is far enough inside the quad, such that the pixels are
    // not affected by the straight border.
    let is_within_inner_straight_border =
        straight_border_inner_corner_to_point.x < -antialias_threshold &&
        straight_border_inner_corner_to_point.y < -antialias_threshold;

    // Fast path for points that must be part of the background.
    //
    // This could be optimized further for large rounded corners by including
    // points in an inscribed rectangle, or some other quick linear check.
    // However, that might negatively impact performance in the case of
    // reasonable sizes for rounded corners.
    if (is_within_inner_straight_border && !is_near_rounded_corner) {
        return blend_color(background_color, 1.0);
    }

    // Signed distance of the point to the outside edge of the quad's border. It
    // is positive outside this edge, and negative inside.
    var outer_sdf = quad_sdf_impl(corner_center_to_point, corner_radius);
    if (corner_exponent != 2.0) {
        outer_sdf = squircle_sdf(corner_center_to_point, corner_radius, corner_exponent);
    }

    // Approximate signed distance of the point to the inside edge of the quad's
    // border. It is negative outside this edge (within the border), and
    // positive inside.
    //
    // This is not always an accurate signed distance:
    // * The rounded portions with varying border width use an approximation of
    //   nearest-point-on-ellipse.
    // * When it is quickly known to be outside the edge, -1.0 is used.
    var inner_sdf = 0.0;
    if (corner_center_to_point.x <= 0 || corner_center_to_point.y <= 0) {
        // Fast paths for straight borders.
        inner_sdf = -max(straight_border_inner_corner_to_point.x,
                         straight_border_inner_corner_to_point.y);
    } else if (is_beyond_inner_straight_border) {
        // Fast path for points that must be outside the inner edge.
        inner_sdf = -1.0;
    } else if (reduced_border.x == reduced_border.y) {
        // Fast path for circular inner edge.
        inner_sdf = -(outer_sdf + reduced_border.x);
    } else {
        let ellipse_radii = max(vec2<f32>(0.0), corner_radius - reduced_border);
        if (corner_exponent == 2.0) {
            inner_sdf = quarter_ellipse_sdf(corner_center_to_point, ellipse_radii);
        } else {
            inner_sdf = -squircle_corner_sdf(corner_center_to_point, ellipse_radii, corner_exponent);
        }
    }

    // Negative when inside the border
    let border_sdf = max(inner_sdf, outer_sdf);

    var color = background_color;
    if (border_sdf < antialias_threshold) {
        var border_color = input.border_color;

        // Dashed border logic when border_style == 1
        if (quad.border_style == 1) {
            // Position along the perimeter in "dash space", where each dash
            // period has length 1
            var t = 0.0;

            // Total number of dash periods, so that the dash spacing can be
            // adjusted to evenly divide it
            var max_t = 0.0;

            // Border width is proportional to dash size. This is the behavior
            // used by browsers, but also avoids dashes from different segments
            // overlapping when dash size is smaller than the border width.
            //
            // Dash pattern: (2 * border width) dash, (1 * border width) gap
            let dash_length_per_width = 2.0;
            let dash_gap_per_width = 1.0;
            let dash_period_per_width = dash_length_per_width + dash_gap_per_width;

            // Since the dash size is determined by border width, the density of
            // dashes varies. Multiplying a pixel distance by this returns a
            // position in dash space - it has units (dash period / pixels). So
            // a dash velocity of (1 / 10) is 1 dash every 10 pixels.
            var dash_velocity = 0.0;

            // Dividing this by the border width gives the dash velocity
            let dv_numerator = 1.0 / dash_period_per_width;

            if (unrounded) {
                // When corners aren't rounded, the dashes are separately laid
                // out on each straight line, rather than around the whole
                // perimeter. This way each line starts and ends with a dash.
                let is_horizontal =
                        corner_center_to_point.x <
                        corner_center_to_point.y;

                // When applying dashed borders to just some, not all, the sides.
                // The way we chose border widths above sometimes comes with a 0 width value.
                // So we choose again to avoid division by zero.
                // TODO: A better solution exists taking a look at the whole file.
                // this does not fix single dashed borders at the corners
                let dashed_border = vec2<f32>(
                        max(
                            quad.border_widths.bottom,
                            quad.border_widths.top,
                        ),
                        max(
                            quad.border_widths.right,
                            quad.border_widths.left,
                        )
                   );

                let border_width = select(dashed_border.y, dashed_border.x, is_horizontal);
                dash_velocity = dv_numerator / border_width;
                t = select(point.y, point.x, is_horizontal) * dash_velocity;
                max_t = select(size.y, size.x, is_horizontal) * dash_velocity;
            } else {
                // When corners are rounded, the dashes are laid out clockwise
                // around the whole perimeter.

                let r_tr = quad.corner_radii.top_right;
                let r_br = quad.corner_radii.bottom_right;
                let r_bl = quad.corner_radii.bottom_left;
                let r_tl = quad.corner_radii.top_left;

                let w_t = quad.border_widths.top;
                let w_r = quad.border_widths.right;
                let w_b = quad.border_widths.bottom;
                let w_l = quad.border_widths.left;

                // Straight side dash velocities
                let dv_t = select(dv_numerator / w_t, 0.0, w_t <= 0.0);
                let dv_r = select(dv_numerator / w_r, 0.0, w_r <= 0.0);
                let dv_b = select(dv_numerator / w_b, 0.0, w_b <= 0.0);
                let dv_l = select(dv_numerator / w_l, 0.0, w_l <= 0.0);

                // Straight side lengths in dash space
                let s_t = (size.x - r_tl - r_tr) * dv_t;
                let s_r = (size.y - r_tr - r_br) * dv_r;
                let s_b = (size.x - r_br - r_bl) * dv_b;
                let s_l = (size.y - r_bl - r_tl) * dv_l;

                let corner_dash_velocity_tr = corner_dash_velocity(dv_t, dv_r);
                let corner_dash_velocity_br = corner_dash_velocity(dv_b, dv_r);
                let corner_dash_velocity_bl = corner_dash_velocity(dv_b, dv_l);
                let corner_dash_velocity_tl = corner_dash_velocity(dv_t, dv_l);

                // Corner lengths in dash space
                let c_tr = r_tr * (M_PI_F / 2.0) * corner_dash_velocity_tr;
                let c_br = r_br * (M_PI_F / 2.0) * corner_dash_velocity_br;
                let c_bl = r_bl * (M_PI_F / 2.0) * corner_dash_velocity_bl;
                let c_tl = r_tl * (M_PI_F / 2.0) * corner_dash_velocity_tl;

                // Cumulative dash space upto each segment
                let upto_tr = s_t;
                let upto_r = upto_tr + c_tr;
                let upto_br = upto_r + s_r;
                let upto_b = upto_br + c_br;
                let upto_bl = upto_b + s_b;
                let upto_l = upto_bl + c_bl;
                let upto_tl = upto_l + s_l;
                max_t = upto_tl + c_tl;

                if (is_near_rounded_corner) {
                    let radians = atan2(corner_center_to_point.y,
                                        corner_center_to_point.x);
                    let corner_t = radians * corner_radius;

                    if (center_to_point.x >= 0.0) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = corner_dash_velocity_tr;
                            // Subtracted because radians is pi/2 to 0 when
                            // going clockwise around the top right corner,
                            // since the y axis has been flipped
                            t = upto_r - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_br;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the bottom-right corner
                            t = upto_br + corner_t * dash_velocity;
                        }
                    } else {
                        if (center_to_point.y >= 0.0) {
                            dash_velocity = corner_dash_velocity_bl;
                            // Subtracted because radians is pi/2 to 0 when
                            // going clockwise around the bottom-left corner,
                            // since the x axis has been flipped
                            t = upto_l - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_tl;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the top-left corner, since both
                            // axis were flipped
                            t = upto_tl + corner_t * dash_velocity;
                        }
                    }
                } else {
                    // Straight borders
                    let is_horizontal =
                            corner_center_to_point.x <
                            corner_center_to_point.y;
                    if (is_horizontal) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = dv_t;
                            t = (point.x - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_b;
                            t = upto_bl - (point.x - r_bl) * dash_velocity;
                        }
                    } else {
                        if (center_to_point.x < 0.0) {
                            dash_velocity = dv_l;
                            t = upto_tl - (point.y - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_r;
                            t = upto_r + (point.y - r_tr) * dash_velocity;
                        }
                    }
                }
            }

            let dash_length = dash_length_per_width / dash_period_per_width;
            let desired_dash_gap = dash_gap_per_width / dash_period_per_width;

            // Straight borders should start and end with a dash, so max_t is
            // reduced to cause this.
            max_t -= select(0.0, dash_length, unrounded);
            if (max_t >= 1.0) {
                // Adjust dash gap to evenly divide max_t.
                let dash_count = floor(max_t);
                let dash_period = max_t / dash_count;
                border_color.a *= dash_alpha(
                    t,
                    dash_period,
                    dash_length,
                    dash_velocity,
                    antialias_threshold);
            } else if (unrounded) {
                // When there isn't enough space for the full gap between the
                // two start / end dashes of a straight border, reduce gap to
                // make them fit.
                let dash_gap = max_t - dash_length;
                if (dash_gap > 0.0) {
                    let dash_period = dash_length + dash_gap;
                    border_color.a *= dash_alpha(
                        t,
                        dash_period,
                        dash_length,
                        dash_velocity,
                        antialias_threshold);
                }
            }
        }

        // Blend the border on top of the background and then linearly interpolate
        // between the two as we slide inside the background.
        let blended_border = over(background_color, border_color);
        color = mix(background_color, blended_border,
                    saturate(antialias_threshold - inner_sdf));
    }

    return blend_color(color, saturate(antialias_threshold - outer_sdf));
}

// Returns the dash velocity of a corner given the dash velocity of the two
// sides, by returning the slower velocity (larger dashes).
//
// Since 0 is used for dash velocity when the border width is 0 (instead of
// +inf), this returns the other dash velocity in that case.
//
// An alternative to this might be to appropriately interpolate the dash
// velocity around the corner, but that seems overcomplicated.
fn corner_dash_velocity(dv1: f32, dv2: f32) -> f32 {
    if (dv1 == 0.0) {
        return dv2;
    } else if (dv2 == 0.0) {
        return dv1;
    } else {
        return min(dv1, dv2);
    }
}

// Returns alpha used to render antialiased dashes.
// `t` is within the dash when `fmod(t, period) < length`.
fn dash_alpha(t: f32, period: f32, length: f32, dash_velocity: f32, antialias_threshold: f32) -> f32 {
    let half_period = period / 2;
    let half_length = length / 2;
    // Value in [-half_period, half_period].
    // The dash is in [-half_length, half_length].
    let centered = fmod(t + half_period - half_length, period) - half_period;
    // Signed distance for the dash, negative values are inside the dash.
    let signed_distance = abs(centered) - half_length;
    // Antialiased alpha based on the signed distance.
    return saturate(antialias_threshold - signed_distance / dash_velocity);
}

// This approximates distance to the nearest point to a quarter ellipse in a way
// that is sufficient for anti-aliasing when the ellipse is not very eccentric.
// The components of `point` are expected to be positive.
//
// Negative on the outside and positive on the inside.
fn quarter_ellipse_sdf(point: vec2<f32>, radii: vec2<f32>) -> f32 {
    // Scale the space to treat the ellipse like a unit circle.
    let circle_vec = point / radii;
    let unit_circle_sdf = length(circle_vec) - 1.0;
    // Approximate up-scaling of the length by using the average of the radii.
    //
    // TODO: A better solution would be to use the gradient of the implicit
    // function for an ellipse to approximate a scaling factor.
    return unit_circle_sdf * (radii.x + radii.y) * -0.5;
}

// Modulus that has the same sign as `a`.
fn fmod(a: f32, b: f32) -> f32 {
    return a - b * trunc(a / b);
}

// --- shadows --- //

struct Shadow {
    order: u32,
    blur_radius: f32,
    bounds: Bounds,
    corner_radii: Corners,
    content_mask: Bounds,
    color: Hsla,
}
var<storage, read> b_shadows: array<Shadow>;

struct ShadowVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) @interpolate(flat) shadow_id: u32,
    //TODO: use `clip_distance` once Naga supports it
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_shadow(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> ShadowVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    var shadow = b_shadows[instance_id];

    let margin = 3.0 * shadow.blur_radius;
    // Set the bounds of the shadow and adjust its size based on the shadow's
    // spread radius to achieve the spreading effect
    shadow.bounds.origin -= vec2<f32>(margin);
    shadow.bounds.size += 2.0 * vec2<f32>(margin);

    var out = ShadowVarying();
    out.position = to_device_position(unit_vertex, shadow.bounds);
    out.color = hsla_to_rgba(shadow.color);
    out.shadow_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, shadow.bounds, shadow.content_mask);
    return out;
}

@fragment
fn fs_shadow(input: ShadowVarying) -> @location(0) vec4<f32> {
    // Alpha clip first, since we don't have `clip_distance`.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let shadow = b_shadows[input.shadow_id];
    let half_size = shadow.bounds.size / 2.0;
    let center = shadow.bounds.origin + half_size;
    let center_to_point = input.position.xy - center;

    let corner_radius = pick_corner_radius(center_to_point, shadow.corner_radii);

    // The signal is only non-zero in a limited range, so don't waste samples
    let low = center_to_point.y - half_size.y;
    let high = center_to_point.y + half_size.y;
    let start = clamp(-3.0 * shadow.blur_radius, low, high);
    let end = clamp(3.0 * shadow.blur_radius, low, high);

    // Accumulate samples (we can get away with surprisingly few samples)
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var alpha = 0.0;
    for (var i = 0; i < 4; i += 1) {
        let blur = blur_along_x(center_to_point.x, center_to_point.y - y,
            shadow.blur_radius, corner_radius, half_size);
        alpha +=  blur * gaussian(y, shadow.blur_radius) * step;
        y += step;
    }

    return blend_color(input.color, alpha);
}

// --- Plays: visualizers, each a quad drawn by a shader of its own, which its `kind` names (see `Visualizer`) --- //

const VISUALIZER_SPEAKER: u32 = 0u;

struct Visualizer {
    order: u32,
    kind: u32,
    bounds: Bounds,
    content_mask: Bounds,
    color: Hsla,
    params: array<f32, 4>,
}
var<storage, read> b_visualizers: array<Visualizer>;

struct VisualizerVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) @interpolate(flat) visualizer_id: u32,
    //TODO: use `clip_distance` once Naga supports it
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_visualizer(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> VisualizerVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let visualizer = b_visualizers[instance_id];

    var out = VisualizerVarying();
    out.position = to_device_position(unit_vertex, visualizer.bounds);
    out.color = hsla_to_rgba(visualizer.color);
    out.visualizer_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, visualizer.bounds, visualizer.content_mask);
    return out;
}

// How much of the pixel `distance` from the middle a ring of `radius`
// covers: `width` wide, its edges blurred over `blur` either side.
fn ring(distance: f32, radius: f32, width: f32, blur: f32) -> f32 {
    let x = distance - radius;
    return smoothstep(-blur, blur, x + width / 2.0) - smoothstep(-blur, blur, x - width / 2.0);
}

// A speaker seen from the front, in the middle of `bounds`, as wide as
// fits: two thin rings, blurred over a point (`scale` device pixels),
// which move out with `level`, from 0 to 1 (as `VisualizerShader::encode`
// clamps it): its basket's rim, a point wide, and inside it its cone's
// edge, two points wide, which moves twice as far. How much of the pixel
// at `position` it covers.
fn speaker(position: vec2<f32>, bounds: Bounds, level: f32, scale: f32) -> f32 {
    let center = bounds.origin + bounds.size / 2.0;
    let radius = max(0.46 * min(bounds.size.x, bounds.size.y), 1.0);
    let distance = length(position - center);
    let blur = max(scale, 0.5);
    let rim = ring(distance, (0.96 + 0.04 * level) * radius, scale, blur);
    let cone = ring(distance, (0.84 + 0.08 * level) * radius, 2.0 * scale, blur);
    return max(rim, cone);
}

@fragment
fn fs_visualizer(input: VisualizerVarying) -> @location(0) vec4<f32> {
    // Alpha clip first, since we don't have `clip_distance`.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let visualizer = b_visualizers[input.visualizer_id];
    var coverage = 0.0;
    switch (visualizer.kind) {
        case VISUALIZER_SPEAKER, default: {
            coverage = speaker(input.position.xy, visualizer.bounds, visualizer.params[0], visualizer.params[1]);
        }
    }
    return blend_color(input.color, coverage);
}

// --- path rasterization --- //

struct PathRasterizationVertex {
    xy_position: vec2<f32>,
    st_position: vec2<f32>,
    color: Background,
    bounds: Bounds,
}

var<storage, read> b_path_vertices: array<PathRasterizationVertex>;

struct PathRasterizationVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) st_position: vec2<f32>,
    @location(1) vertex_id: u32,
    //TODO: use `clip_distance` once Naga supports it
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_path_rasterization(@builtin(vertex_index) vertex_id: u32) -> PathRasterizationVarying {
    let v = b_path_vertices[vertex_id];

    var out = PathRasterizationVarying();
    out.position = to_device_position_impl(v.xy_position);
    out.st_position = v.st_position;
    out.vertex_id = vertex_id;
    out.clip_distances = distance_from_clip_rect_impl(v.xy_position, v.bounds);
    return out;
}

@fragment
fn fs_path_rasterization(input: PathRasterizationVarying) -> @location(0) vec4<f32> {
    let dx = dpdx(input.st_position);
    let dy = dpdy(input.st_position);
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let v = b_path_vertices[input.vertex_id];
    let background = v.color;
    let bounds = v.bounds;

    var alpha: f32;
    if (length(vec2<f32>(dx.x, dy.x)) < 0.001) {
        // If the gradient is too small, return a solid color.
        alpha = 1.0;
    } else {
        let gradient = 2.0 * input.st_position.xx * vec2<f32>(dx.x, dy.x) - vec2<f32>(dx.y, dy.y);
        let f = input.st_position.x * input.st_position.x - input.st_position.y;
        let distance = f / length(gradient);
        alpha = saturate(0.5 - distance);
    }
    let gradient_color = prepare_gradient_color(
        background.tag,
        background.color_space,
        background.solid,
        background.colors,
    );
    let color = gradient_color(background, input.position.xy, bounds,
        gradient_color.solid, gradient_color.color0, gradient_color.color1);
    return vec4<f32>(color.rgb * color.a * alpha, color.a * alpha);
}

// --- paths --- //

struct PathSprite {
    bounds: Bounds,
}
var<storage, read> b_path_sprites: array<PathSprite>;

struct PathVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) texture_coords: vec2<f32>,
}

@vertex
fn vs_path(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> PathVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let sprite = b_path_sprites[instance_id];
    // Don't apply content mask because it was already accounted for when rasterizing the path.
    let device_position = to_device_position(unit_vertex, sprite.bounds);
    // For screen-space intermediate texture, convert screen position to texture coordinates
    let screen_position = sprite.bounds.origin + unit_vertex * sprite.bounds.size;
    let texture_coords = screen_position / globals.viewport_size;

    var out = PathVarying();
    out.position = device_position;
    out.texture_coords = texture_coords;

    return out;
}

@fragment
fn fs_path(input: PathVarying) -> @location(0) vec4<f32> {
    let sample = textureSample(t_sprite, s_sprite, input.texture_coords);
    return sample;
}

// --- underlines --- //

struct Underline {
    order: u32,
    pad: u32,
    bounds: Bounds,
    content_mask: Bounds,
    color: Hsla,
    thickness: f32,
    wavy: u32,
}
var<storage, read> b_underlines: array<Underline>;

struct UnderlineVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) @interpolate(flat) underline_id: u32,
    //TODO: use `clip_distance` once Naga supports it
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_underline(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> UnderlineVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let underline = b_underlines[instance_id];

    var out = UnderlineVarying();
    out.position = to_device_position(unit_vertex, underline.bounds);
    out.color = hsla_to_rgba(underline.color);
    out.underline_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, underline.bounds, underline.content_mask);
    return out;
}

@fragment
fn fs_underline(input: UnderlineVarying) -> @location(0) vec4<f32> {
    const WAVE_FREQUENCY: f32 = 2.0;
    const WAVE_HEIGHT_RATIO: f32 = 0.8;

    // Alpha clip first, since we don't have `clip_distance`.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let underline = b_underlines[input.underline_id];
    if ((underline.wavy & 0xFFu) == 0u)
    {
        return blend_color(input.color, input.color.a);
    }

    let half_thickness = underline.thickness * 0.5;

    let st = (input.position.xy - underline.bounds.origin) / underline.bounds.size.y - vec2<f32>(0.0, 0.5);
    let frequency = M_PI_F * WAVE_FREQUENCY * underline.thickness / underline.bounds.size.y;
    let amplitude = (underline.thickness * WAVE_HEIGHT_RATIO) / underline.bounds.size.y;

    let sine = sin(st.x * frequency) * amplitude;
    let dSine = cos(st.x * frequency) * amplitude * frequency;
    let distance = (st.y - sine) / sqrt(1.0 + dSine * dSine);
    let distance_in_pixels = distance * underline.bounds.size.y;
    let distance_from_top_border = distance_in_pixels - half_thickness;
    let distance_from_bottom_border = distance_in_pixels + half_thickness;
    let alpha = saturate(0.5 - max(-distance_from_bottom_border, distance_from_top_border));
    return blend_color(input.color, alpha * input.color.a);
}

// --- monochrome sprites --- //

struct MonochromeSprite {
    order: u32,
    pad: u32,
    bounds: Bounds,
    content_mask: Bounds,
    color: Hsla,
    tile: AtlasTile,
    transformation: TransformationMatrix,
}
var<storage, read> b_mono_sprites: array<MonochromeSprite>;

struct MonoSpriteVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) tile_position: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_mono_sprite(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> MonoSpriteVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let sprite = b_mono_sprites[instance_id];

    var out = MonoSpriteVarying();
    out.position = to_device_position_transformed(unit_vertex, sprite.bounds, sprite.transformation);

    out.tile_position = to_tile_position(unit_vertex, sprite.tile);
    out.color = hsla_to_rgba(sprite.color);
    out.clip_distances = distance_from_clip_rect_transformed(unit_vertex, sprite.bounds, sprite.content_mask, sprite.transformation);
    return out;
}

@fragment
fn fs_mono_sprite(input: MonoSpriteVarying) -> @location(0) vec4<f32> {
    let sample = textureSample(t_sprite, s_sprite, input.tile_position).r;
    let alpha_corrected = apply_contrast_and_gamma_correction(sample, input.color.rgb, grayscale_enhanced_contrast, gamma_ratios);

    // Alpha clip after using the derivatives.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    // convert to srgb space as the rest of the code (output swapchain) expects that
    return blend_color(input.color, alpha_corrected);
}

// --- polychrome sprites --- //

struct PolychromeSprite {
    order: u32,
    pad: u32,
    grayscale: u32,
    opacity: f32,
    bounds: Bounds,
    content_mask: Bounds,
    corner_radii: Corners,
    tile: AtlasTile,
    // Plays: see `PolychromeSprite::corner_smoothing`.
    corner_smoothing: f32,
    smoothing_pad: u32,
    // Plays: see `PolychromeSprite::content`.
    content: ContentTransform,
}
var<storage, read> b_poly_sprites: array<PolychromeSprite>;

// Plays: see `ContentTransform`.
struct ContentTransform {
    origin: vec2<f32>,
    across: vec2<f32>,
    down: vec2<f32>,
}

// Plays: where a point of a sprite, in fractions of its size, samples its
// texture.
fn content_position(unit_vertex: vec2<f32>, content: ContentTransform) -> vec2<f32> {
    return content.origin + unit_vertex.x * content.across + unit_vertex.y * content.down;
}

struct PolySpriteVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) tile_position: vec2<f32>,
    @location(1) @interpolate(flat) sprite_id: u32,
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_poly_sprite(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> PolySpriteVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let sprite = b_poly_sprites[instance_id];

    var out = PolySpriteVarying();
    out.position = to_device_position(unit_vertex, sprite.bounds);
    // Plays: the part of the image shown, turned or mirrored.
    out.tile_position = to_tile_position(content_position(unit_vertex, sprite.content), sprite.tile);
    out.sprite_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, sprite.bounds, sprite.content_mask);
    return out;
}

@fragment
fn fs_poly_sprite(input: PolySpriteVarying) -> @location(0) vec4<f32> {
    let sample = textureSample(t_sprite, s_sprite, input.tile_position);
    // Alpha clip after using the derivatives.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let sprite = b_poly_sprites[input.sprite_id];
    let distance = quad_sdf_smoothed(input.position.xy, sprite.bounds,
                                     sprite.corner_radii, sprite.corner_smoothing);

    var color = sample;
    if ((sprite.grayscale & 0xFFu) != 0u) {
        let grayscale = dot(color.rgb, GRAYSCALE_FACTORS);
        color = vec4<f32>(vec3<f32>(grayscale), sample.a);
    }
    return blend_color(color, sprite.opacity * saturate(0.5 - distance));
}

// --- surfaces --- //

struct SurfaceParams {
    bounds: Bounds,
    content_mask: Bounds,
}

var<uniform> surface_locals: SurfaceParams;
var t_y: texture_2d<f32>;
var t_cb_cr: texture_2d<f32>;
var s_surface: sampler;

const ycbcr_to_RGB = mat4x4<f32>(
    vec4<f32>( 1.0000f,  1.0000f,  1.0000f, 0.0),
    vec4<f32>( 0.0000f, -0.3441f,  1.7720f, 0.0),
    vec4<f32>( 1.4020f, -0.7141f,  0.0000f, 0.0),
    vec4<f32>(-0.7010f,  0.5291f, -0.8860f, 1.0),
);

struct SurfaceVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_surface(@builtin(vertex_index) vertex_id: u32) -> SurfaceVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));

    var out = SurfaceVarying();
    out.position = to_device_position(unit_vertex, surface_locals.bounds);
    out.texture_position = unit_vertex;
    out.clip_distances = distance_from_clip_rect(unit_vertex, surface_locals.bounds, surface_locals.content_mask);
    return out;
}

@fragment
fn fs_surface(input: SurfaceVarying) -> @location(0) vec4<f32> {
    // Alpha clip after using the derivatives.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let y_cb_cr = vec4<f32>(
        textureSampleLevel(t_y, s_surface, input.texture_position, 0.0).r,
        textureSampleLevel(t_cb_cr, s_surface, input.texture_position, 0.0).rg,
        1.0);

    return ycbcr_to_RGB * y_cb_cr;
}

// --- Plays: video frames' planes, converted as they are drawn (see PLAYS.md) --- //

// Matches YuvParams in blade_yuv.rs.
struct YuvParams {
    bounds: Bounds,
    content_mask: Bounds,
    // ContentTransform's origin and across, then its down.
    content_origin_across: vec4<f32>,
    content_down: vec4<f32>,
    // Columns from samples (y, cb, cr, 1) to nonlinear R'G'B'.
    ycbcr_to_rgb: mat4x4<f32>,
    // Columns from the video's linear RGB to sRGB's.
    to_srgb_primaries: mat4x4<f32>,
    // 0 for SDR, 1 for PQ, 2 for HLG.
    transfer: u32,
    gamma: f32,
    // Whether Cb and Cr are planes of their own, rather than one.
    planar: u32,
    // Whether samples are 16 bits, two bytes of a texel each.
    wide: u32,
    // Where the first Cb and Cr samples sit, in luma samples from the
    // top-left corner (xy).
    chroma_siting: vec4<f32>,
    // The brightest the video gets, in nits; 0 when it isn't known.
    peak_nits: f32,
}

var<uniform> yuv_params: YuvParams;
var t_yuv_y: texture_2d<f32>;
var t_yuv_cb: texture_2d<f32>;
var t_yuv_cr: texture_2d<f32>;
var s_yuv: sampler;

struct YuvVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) texture_position: vec2<f32>,
    @location(3) clip_distances: vec4<f32>,
}

@vertex
fn vs_yuv(@builtin(vertex_index) vertex_id: u32) -> YuvVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let content = ContentTransform(
        yuv_params.content_origin_across.xy,
        yuv_params.content_origin_across.zw,
        yuv_params.content_down.xy,
    );
    var out = YuvVarying();
    out.position = to_device_position(unit_vertex, yuv_params.bounds);
    out.texture_position = content_position(unit_vertex, content);
    out.clip_distances = distance_from_clip_rect(unit_vertex, yuv_params.bounds, yuv_params.content_mask);
    return out;
}

// A little-endian 16-bit sample from its two bytes, as a fraction of 65535.
fn yuv_wide_sample(bytes: vec2<f32>) -> f32 {
    return (bytes.y * 65280.0 + bytes.x * 255.0) / 65535.0;
}

// The 16-bit samples in a texel: one in red and green, a second in blue and
// alpha.
fn yuv_wide_texel(t: texture_2d<f32>, position: vec2<i32>) -> vec2<f32> {
    let bytes = textureLoad(t, position, 0);
    return vec2<f32>(yuv_wide_sample(bytes.rg), yuv_wide_sample(bytes.ba));
}

// Bilinear filtering by hand: filtering the bytes apart would blend high and
// low bytes.
fn yuv_sample_wide(t: texture_2d<f32>, position: vec2<f32>) -> vec2<f32> {
    let size = vec2<i32>(textureDimensions(t, 0));
    let texel = position * vec2<f32>(size) - 0.5;
    let base = floor(texel);
    let f = texel - base;
    let last = size - vec2<i32>(1);
    let p0 = clamp(vec2<i32>(base), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(base) + vec2<i32>(1), vec2<i32>(0), last);
    let top = mix(yuv_wide_texel(t, p0), yuv_wide_texel(t, vec2<i32>(p1.x, p0.y)), f.x);
    let bottom = mix(yuv_wide_texel(t, vec2<i32>(p0.x, p1.y)), yuv_wide_texel(t, p1), f.x);
    return mix(top, bottom, f.y);
}

// A plane's samples at `position`: the first, or Cb and Cr interleaved.
fn yuv_sample(t: texture_2d<f32>, position: vec2<f32>) -> vec2<f32> {
    if (yuv_params.wide != 0u) {
        return yuv_sample_wide(t, position);
    }
    return textureSampleLevel(t, s_yuv, position, 0.0).rg;
}

// Where `position` falls in a chroma plane, whose samples sit at
// chroma_siting + 2k in luma samples, rather than at the middle of each
// 2x2 block of them.
fn yuv_chroma_position(position: vec2<f32>, chroma: texture_2d<f32>) -> vec2<f32> {
    let luma = position * vec2<f32>(textureDimensions(t_yuv_y, 0));
    let texel = (luma - yuv_params.chroma_siting.xy) * 0.5 + 0.5;
    return texel / vec2<f32>(textureDimensions(chroma, 0));
}

// Gamma, which bends the encoded signal (Plays' apply_gamma).
fn yuv_apply_gamma(signal: vec3<f32>, gamma: f32) -> vec3<f32> {
    if (gamma == 1.0) {
        return signal;
    }
    return sign(signal) * pow(abs(signal), vec3<f32>(1.0 / gamma));
}

// HDR to an SDR screen: the same math and constants as Plays' crates/hdr,
// whose tests read them here.

const YUV_REFERENCE_WHITE_NITS: f32 = 203.0;
const YUV_HLG_PEAK_NITS: f32 = 1000.0;
const YUV_HLG_GAMMA: f32 = 1.2;
const YUV_TONE_KNEE: f32 = 0.75;

fn yuv_pq_to_nits(signal: vec3<f32>) -> vec3<f32> {
    let m1 = 2610.0 / 16384.0;
    let m2 = 2523.0 / 4096.0 * 128.0;
    let c1 = 3424.0 / 4096.0;
    let c2 = 2413.0 / 4096.0 * 32.0;
    let c3 = 2392.0 / 4096.0 * 32.0;
    let p = pow(signal, vec3<f32>(1.0 / m2));
    return 10000.0 * pow(max(p - c1, vec3<f32>(0.0)) / (c2 - c3 * p), vec3<f32>(1.0 / m1));
}

fn yuv_hlg_to_nits(signal: vec3<f32>) -> vec3<f32> {
    let a = 0.17883277;
    let b = 1.0 - 4.0 * a;
    let c = 0.5599107;
    let scene = select((exp((signal - c) / a) + b) / 12.0, signal * signal / 3.0, signal <= vec3<f32>(0.5));
    let luma = dot(scene, vec3<f32>(0.2627, 0.6780, 0.0593));
    return YUV_HLG_PEAK_NITS * pow(max(luma, 0.0), YUV_HLG_GAMMA - 1.0) * scene;
}

// The brightest channel is rolled off toward SDR white past the knee,
// reaching it at the video's peak (relative to SDR white) if that's known
// (above 0), and the others scale with it.
fn yuv_tone_map(rgb: vec3<f32>, peak: f32) -> vec3<f32> {
    let brightest = max(rgb.r, max(rgb.g, rgb.b));
    if (brightest <= YUV_TONE_KNEE) {
        return rgb;
    }
    let room = 1.0 - YUV_TONE_KNEE;
    let past = (brightest - YUV_TONE_KNEE) / room;
    var eased = 1.0 - exp(-past);
    if (peak > 0.0) {
        let white = max((peak - YUV_TONE_KNEE) / room, 1.0);
        eased = min(past * (1.0 + past / (white * white)) / (1.0 + past), 1.0);
    }
    return rgb * ((YUV_TONE_KNEE + room * eased) / brightest);
}

fn yuv_srgb_encode(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055, 12.92 * linear, linear <= vec3<f32>(0.0031308));
}

@fragment
fn fs_yuv(input: YuvVarying) -> @location(0) vec4<f32> {
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }
    let position = input.texture_position;
    let y = yuv_sample(t_yuv_y, position).x;
    let chroma = yuv_chroma_position(position, t_yuv_cb);
    var cb_cr: vec2<f32>;
    if (yuv_params.planar != 0u) {
        cb_cr = vec2<f32>(yuv_sample(t_yuv_cb, chroma).x, yuv_sample(t_yuv_cr, chroma).x);
    } else {
        cb_cr = yuv_sample(t_yuv_cb, chroma);
    }
    let signal = saturate((yuv_params.ycbcr_to_rgb * vec4<f32>(y, cb_cr, 1.0)).rgb);

    var encoded: vec3<f32>;
    if (yuv_params.transfer == 0u) {
        encoded = signal;
    } else {
        var light: vec3<f32>;
        if (yuv_params.transfer == 1u) {
            light = yuv_pq_to_nits(signal) / YUV_REFERENCE_WHITE_NITS;
        } else {
            light = yuv_hlg_to_nits(signal) / YUV_REFERENCE_WHITE_NITS;
        }
        let rgb = (yuv_params.to_srgb_primaries * vec4<f32>(light, 0.0)).rgb;
        let peak = yuv_params.peak_nits / YUV_REFERENCE_WHITE_NITS;
        encoded = yuv_srgb_encode(saturate(yuv_tone_map(rgb, peak)));
    }
    return blend_color(vec4<f32>(yuv_apply_gamma(encoded, yuv_params.gamma), 1.0), 1.0);
}

// --- backdrops --- //
// Plays: see `Backdrop` in scene.rs, and the same shaders in shaders.metal.
// What is drawn under a batch of backdrops is halved six times
// (`BACKDROP_LEVELS`), each level from the one before, by `*_backdrop_pass`;
// `*_backdrop` then draws each one from those levels.

// The last level, a 64th of the viewport: `BACKDROP_LEVELS` - 1.
const BACKDROP_LAST_LEVEL: u32 = 5u;
const BACKDROP_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// How strongly the colors around glow inside the edge, and outside it.
const BACKDROP_GLOW_INSIDE: f32 = 0.6;
const BACKDROP_GLOW_OUTSIDE: f32 = 0.6;
// Glass's index of refraction, and how far its bevel bends light at most,
// a part of the bevel's width (see `backdrop_bend`).
const BACKDROP_IOR: f32 = 1.5;
const BACKDROP_BEND_PEAK: f32 = 0.23126;

// What a pass makes, in the viewport's pixels: `region` (origin, size),
// and `texels`: a texel of the source, a texel of the level it makes, and
// that level's size in texels. As `BackdropPassParams` in Rust.
struct BackdropPassParams {
    region: vec4<f32>,
    texels: vec4<f32>,
}
var<uniform> backdrop_pass: BackdropPassParams;
var t_backdrop_source: texture_2d<f32>;
var s_backdrop: sampler;

struct BackdropPassVarying {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_backdrop_pass(@builtin(vertex_index) vertex_id: u32) -> BackdropPassVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let texel = backdrop_pass.texels.y;
    let size = backdrop_pass.texels.zw;
    // The region in the level's texels.
    let low = clamp(floor(backdrop_pass.region.xy / texel), vec2<f32>(0.0), size);
    let high = clamp(ceil((backdrop_pass.region.xy + backdrop_pass.region.zw) / texel),
                     vec2<f32>(0.0), size);
    let position = mix(low, high, unit_vertex);
    var out = BackdropPassVarying();
    out.position = vec4<f32>(position / size * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

fn backdrop_tap(point: vec2<f32>, to_uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(t_backdrop_source, s_backdrop, point * to_uv, 0.0);
}

// Halves the source with the 13 taps of Jimenez's "Next generation post
// processing in Call of Duty: Advanced Warfare" (2014): each tap the
// average of four of its texels, weighted toward the middle, so what's
// small and bright doesn't flicker as it moves.
@fragment
fn fs_backdrop_pass(input: BackdropPassVarying) -> @location(0) vec4<f32> {
    let o = backdrop_pass.texels.x;
    // The middle of this texel, in the viewport's pixels.
    let center = input.position.xy * backdrop_pass.texels.y;
    let to_uv = 1.0 / (vec2<f32>(textureDimensions(t_backdrop_source, 0)) * o);
    var color = backdrop_tap(center, to_uv) * 0.125;
    color += (backdrop_tap(center + vec2<f32>(-2.0, -2.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(2.0, -2.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(-2.0, 2.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(2.0, 2.0) * o, to_uv)) * 0.03125;
    color += (backdrop_tap(center + vec2<f32>(0.0, -2.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(-2.0, 0.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(2.0, 0.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(0.0, 2.0) * o, to_uv)) * 0.0625;
    color += (backdrop_tap(center + vec2<f32>(-1.0, -1.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(1.0, -1.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(-1.0, 1.0) * o, to_uv) +
              backdrop_tap(center + vec2<f32>(1.0, 1.0) * o, to_uv)) * 0.125;
    return color;
}

struct Backdrop {
    order: u32,
    pad: u32,
    bounds: Bounds,
    content_mask: Bounds,
    shape: Bounds,
    corner_radii: Corners,
    tint: Hsla,
    corner_smoothing: f32,
    blur: f32,
    saturation: f32,
    brightness: f32,
    average: f32,
    refraction: f32,
    edge: f32,
    rim: f32,
    glow: f32,
    halo: f32,
    opacity: f32,
    pad2: u32,
}
var<storage, read> b_backdrops: array<Backdrop>;
var t_backdrop0: texture_2d<f32>;
var t_backdrop1: texture_2d<f32>;
var t_backdrop2: texture_2d<f32>;
var t_backdrop3: texture_2d<f32>;
var t_backdrop4: texture_2d<f32>;
var t_backdrop5: texture_2d<f32>;
// The frame as it was before the backdrops.
var t_backdrop_frame: texture_2d<f32>;

struct BackdropVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) backdrop_id: u32,
    @location(1) clip_distances: vec4<f32>,
}

@vertex
fn vs_backdrop(@builtin(vertex_index) vertex_id: u32, @builtin(instance_index) instance_id: u32) -> BackdropVarying {
    let unit_vertex = vec2<f32>(f32(vertex_id & 1u), 0.5 * f32(vertex_id & 2u));
    let backdrop = b_backdrops[instance_id];
    var out = BackdropVarying();
    out.position = to_device_position(unit_vertex, backdrop.bounds);
    out.backdrop_id = instance_id;
    out.clip_distances = distance_from_clip_rect(unit_vertex, backdrop.bounds, backdrop.content_mask);
    return out;
}

// A level at `point` (in the viewport's pixels), through a tent over the
// texels around it so they don't show.
fn backdrop_level_sample(level: texture_2d<f32>, index: u32, point: vec2<f32>) -> vec4<f32> {
    let texel = f32(2u << index);
    let to_uv = 1.0 / (vec2<f32>(textureDimensions(level, 0)) * texel);
    var color = textureSampleLevel(level, s_backdrop, point * to_uv, 0.0) * 4.0;
    color += (textureSampleLevel(level, s_backdrop, (point + vec2<f32>(texel, 0.0)) * to_uv, 0.0) +
              textureSampleLevel(level, s_backdrop, (point - vec2<f32>(texel, 0.0)) * to_uv, 0.0) +
              textureSampleLevel(level, s_backdrop, (point + vec2<f32>(0.0, texel)) * to_uv, 0.0) +
              textureSampleLevel(level, s_backdrop, (point - vec2<f32>(0.0, texel)) * to_uv, 0.0)) * 2.0;
    color += textureSampleLevel(level, s_backdrop, (point + vec2<f32>(texel, texel)) * to_uv, 0.0) +
             textureSampleLevel(level, s_backdrop, (point + vec2<f32>(-texel, texel)) * to_uv, 0.0) +
             textureSampleLevel(level, s_backdrop, (point + vec2<f32>(texel, -texel)) * to_uv, 0.0) +
             textureSampleLevel(level, s_backdrop, (point + vec2<f32>(-texel, -texel)) * to_uv, 0.0);
    return color / 16.0;
}

// The frame as it was before the backdrops, at `point`, sharp.
fn backdrop_frame(point: vec2<f32>) -> vec4<f32> {
    let uv = point / vec2<f32>(textureDimensions(t_backdrop_frame, 0));
    return textureSampleLevel(t_backdrop_frame, s_backdrop, uv, 0.0);
}

fn backdrop_level(index: u32, point: vec2<f32>) -> vec4<f32> {
    var color: vec4<f32>;
    switch index {
        case 0u: { color = backdrop_level_sample(t_backdrop0, 0u, point); }
        case 1u: { color = backdrop_level_sample(t_backdrop1, 1u, point); }
        case 2u: { color = backdrop_level_sample(t_backdrop2, 2u, point); }
        case 3u: { color = backdrop_level_sample(t_backdrop3, 3u, point); }
        case 4u: { color = backdrop_level_sample(t_backdrop4, 4u, point); }
        default: { color = backdrop_level_sample(t_backdrop5, 5u, point); }
    }
    return color;
}

// The colors around, as bright and colorful as they can be: what glows.
// Dark ones stay dim.
fn backdrop_vivid(around: vec3<f32>) -> vec3<f32> {
    let color = clamp(around, vec3<f32>(0.0), vec3<f32>(1.0));
    let luma = dot(color, BACKDROP_LUMA);
    let vivid = max(mix(vec3<f32>(luma), color, 1.8), vec3<f32>(0.0));
    let peak = max(max(vivid.r, vivid.g), vivid.b);
    return vivid / max(peak, 0.35) * 0.95;
}

// How far light bends through a bevel of a quarter circle, by Snell's law,
// looking straight down at it: where `slope` of it (1 at the edge, where it
// is steepest, down to 0 where it meets the flat top) shows what's under it
// from, inward, as a part of the farthest it does (`BACKDROP_BEND_PEAK`).
fn backdrop_bend(slope: f32) -> f32 {
    let height = sqrt(max(1.0 - slope * slope, 0.0));
    let bent = asin(slope) - asin(slope / BACKDROP_IOR);
    return height * tan(bent) / BACKDROP_BEND_PEAK;
}

fn backdrop_sdf(point: vec2<f32>, backdrop: Backdrop) -> f32 {
    return quad_sdf_smoothed(point, backdrop.shape, backdrop.corner_radii, backdrop.corner_smoothing);
}

@fragment
fn fs_backdrop(input: BackdropVarying) -> @location(0) vec4<f32> {
    // Alpha clip first, since we don't have `clip_distance`.
    if (any(input.clip_distances < vec4<f32>(0.0))) {
        return vec4<f32>(0.0);
    }

    let backdrop = b_backdrops[input.backdrop_id];
    let point = input.position.xy;
    let sdf = backdrop_sdf(point, backdrop);
    let coverage = saturate(0.5 - sdf);
    let around = backdrop_level(BACKDROP_LAST_LEVEL, point).rgb;
    let vivid = backdrop_vivid(around);

    var halo = 0.0;
    if (backdrop.halo > 0.0 && sdf > 0.0) {
        let t = sdf / backdrop.halo;
        halo = backdrop.glow * BACKDROP_GLOW_OUTSIDE * exp(-4.0 * t * t) * (1.0 - coverage);
    }
    if (coverage <= 0.0 && halo <= 0.0) {
        return vec4<f32>(0.0);
    }

    var color = vec3<f32>(0.0);
    if (coverage > 0.0) {
        // The band along the edge: 1 at the edge, down to 0 `edge` inside.
        var band = 0.0;
        if (backdrop.edge > 0.0) {
            band = saturate(1.0 + sdf / backdrop.edge);
        }
        // Out of the shape, square to its edge.
        var normal = vec2<f32>(0.0);
        if (band > 0.0) {
            let gradient = vec2<f32>(
                backdrop_sdf(point + vec2<f32>(1.0, 0.0), backdrop) -
                    backdrop_sdf(point - vec2<f32>(1.0, 0.0), backdrop),
                backdrop_sdf(point + vec2<f32>(0.0, 1.0), backdrop) -
                    backdrop_sdf(point - vec2<f32>(0.0, 1.0), backdrop));
            let gradient_length = length(gradient);
            if (gradient_length > 0.0001) {
                normal = gradient / gradient_length;
            }
        }
        // Light bends through the bevel as through a rounded glass edge seen
        // from above, showing what's just inside it magnified across it.
        let seen = point - normal * backdrop.refraction * backdrop_bend(band);
        // Which level to read, -1 being the frame itself: the first level's
        // texels are 2 pixels.
        var lod = clamp(log2(max(backdrop.blur, 1.0)) - 1.0, -1.0, f32(BACKDROP_LAST_LEVEL));
        // A bevel that bends light is polished, clear however frosted the top.
        if (backdrop.refraction > 0.0) {
            lod = mix(lod, -1.0, band);
        }
        var under: vec3<f32>;
        if (lod < 0.0) {
            under = mix(backdrop_frame(seen).rgb, backdrop_level(0u, seen).rgb, lod + 1.0);
        } else {
            let below = u32(floor(lod));
            let above = min(below + 1u, BACKDROP_LAST_LEVEL);
            under = mix(backdrop_level(below, seen).rgb, backdrop_level(above, seen).rgb,
                        lod - floor(lod));
        }
        color = mix(under, around, backdrop.average);
        // Brighter than SDR white shows as white.
        color = min(color, vec3<f32>(1.0));
        let luma = dot(color, BACKDROP_LUMA);
        color = max(mix(vec3<f32>(luma), color, backdrop.saturation), vec3<f32>(0.0));
        color *= backdrop.brightness;
        let tint = hsla_to_rgba(backdrop.tint);
        color = mix(color, tint.rgb, tint.a);
        let band2 = band * band;
        color += vivid * backdrop.glow * band2 * BACKDROP_GLOW_INSIDE;
        // The rim of light, brightest where the edge faces the top left or
        // the bottom right.
        let facing = abs(dot(normal, vec2<f32>(-0.70710678, -0.70710678)));
        let rim = backdrop.rim * band2 * band2 * band2 * (0.3 + 0.7 * facing * facing);
        color += mix(vec3<f32>(1.0), vivid, saturate(backdrop.glow)) * rim;
        color = saturate(color);
    }
    let inside = coverage * backdrop.opacity;
    let outside = halo * backdrop.opacity * (1.0 - inside);
    let alpha = inside + outside;
    if (alpha <= 0.0) {
        return vec4<f32>(0.0);
    }
    return blend_color(vec4<f32>((color * inside + vivid * outside) / alpha, alpha), 1.0);
}
