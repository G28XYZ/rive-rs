use std::{fmt, io::Cursor};

use image::io::Reader;
use vello::{
    kurbo::{Affine, BezPath, Cap, Join, Line, PathSeg, Point, Rect, Shape, Stroke, Vec2},
    peniko::{
        self, BlendMode, Brush, BrushRef, Color, ColorStop, ColorStopsSource, Fill, ImageFormat,
        Mix,
    },
    Scene,
};

mod util;

use util::ScaleFromOrigin;

use crate::renderer;

fn to_vello_color(color: renderer::Color) -> Color {
    Color::from_rgba8(color.r, color.g, color.b, color.a)
}

fn to_vello_mix(blend_mode: renderer::BlendMode) -> Mix {
    match blend_mode {
        renderer::BlendMode::SrcOver => Mix::Normal,
        renderer::BlendMode::Screen => Mix::Screen,
        renderer::BlendMode::Overlay => Mix::Overlay,
        renderer::BlendMode::Darken => Mix::Darken,
        renderer::BlendMode::Lighten => Mix::Lighten,
        renderer::BlendMode::ColorDodge => Mix::ColorDodge,
        renderer::BlendMode::ColorBurn => Mix::ColorBurn,
        renderer::BlendMode::HardLight => Mix::HardLight,
        renderer::BlendMode::SoftLight => Mix::SoftLight,
        renderer::BlendMode::Difference => Mix::Difference,
        renderer::BlendMode::Exclusion => Mix::Exclusion,
        renderer::BlendMode::Multiply => Mix::Multiply,
        renderer::BlendMode::Hue => Mix::Hue,
        renderer::BlendMode::Saturation => Mix::Saturation,
        renderer::BlendMode::Color => Mix::Color,
        renderer::BlendMode::Luminosity => Mix::Luminosity,
    }
}

fn triangle_path(points: [Point; 3]) -> BezPath {
    BezPath::from_path_segments(
        [
            PathSeg::Line(Line::new(points[0], points[1])),
            PathSeg::Line(Line::new(points[1], points[2])),
            PathSeg::Line(Line::new(points[2], points[0])),
        ]
        .into_iter(),
    )
}

#[derive(Debug)]
pub struct Buffer {
    // C++ maps these bytes as floats or u16 indices. Keep the allocation aligned
    // for both, including the dangling pointer used by an empty allocation.
    inner: Vec<u32>,
    len_in_bytes: usize,
}

impl Buffer {
    #[inline]
    pub fn as_f32_pairs(&self) -> &[[f32; 2]] {
        bytemuck::cast_slice(&self.bytes()[..self.len_in_bytes])
    }

    #[inline]
    pub fn as_u16s(&self) -> &[u16] {
        bytemuck::cast_slice(&self.bytes()[..self.len_in_bytes])
    }

    fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.inner)
    }
}

impl renderer::Buffer for Buffer {
    #[inline]
    fn new(
        _type: renderer::BufferType,
        _flags: renderer::BufferFlags,
        len_in_bytes: usize,
    ) -> Self {
        Self {
            inner: vec![0; len_in_bytes.div_ceil(size_of::<u32>())],
            len_in_bytes,
        }
    }

    #[inline]
    fn map(&mut self) -> &mut [u8] {
        &mut bytemuck::cast_slice_mut(&mut self.inner)[..self.len_in_bytes]
    }

    #[inline]
    fn unmap(&mut self) {}
}

#[derive(Debug)]
pub struct Path {
    inner: BezPath,
    fill: Fill,
}

impl Default for Path {
    #[inline]
    fn default() -> Self {
        Self {
            inner: Default::default(),
            fill: Fill::NonZero,
        }
    }
}

impl renderer::Path for Path {
    fn new(commands: &mut crate::path::Commands, fill_rule: crate::path::FillRule) -> Self {
        let mut path = Self::default();

        for (verb, points) in commands {
            match verb {
                crate::path::Verb::Move => path.move_to(points[0].x, points[0].y),
                crate::path::Verb::Line => path.line_to(points[0].x, points[0].y),
                crate::path::Verb::Cubic => path.cubic_to(
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y,
                    points[2].x,
                    points[2].y,
                ),
                crate::path::Verb::Close => path.close(),
            }
        }

        path.set_fill_rule(fill_rule);

        path
    }

    #[inline]
    fn reset(&mut self) {
        self.inner.truncate(0);
    }

    #[inline]
    fn extend(&mut self, from: &Self, transform: &[f32; 6]) {
        let mut from = from.inner.clone();
        from.apply_affine(Affine::new(transform.map(Into::into)));

        self.inner.extend(from.elements().iter().cloned());
    }

    #[inline]
    fn set_fill_rule(&mut self, fill_rule: crate::path::FillRule) {
        self.fill = match fill_rule {
            crate::path::FillRule::NonZero => Fill::NonZero,
            crate::path::FillRule::EvenOdd => Fill::EvenOdd,
        };
    }

    #[inline]
    fn move_to(&mut self, x: f32, y: f32) {
        self.inner.move_to(Point::new(x as f64, y as f64));
    }

    #[inline]
    fn line_to(&mut self, x: f32, y: f32) {
        self.inner.line_to(Point::new(x as f64, y as f64));
    }

    #[inline]
    fn cubic_to(&mut self, ox: f32, oy: f32, ix: f32, iy: f32, x: f32, y: f32) {
        self.inner.curve_to(
            Point::new(ox as f64, oy as f64),
            Point::new(ix as f64, iy as f64),
            Point::new(x as f64, y as f64),
        );
    }

    #[inline]
    fn close(&mut self) {
        self.inner.close_path();
    }
}

#[derive(Debug)]
struct SliceStops<'s> {
    colors: &'s [renderer::Color],
    stops: &'s [f32],
}

impl ColorStopsSource for SliceStops<'_> {
    fn collect_stops(self, vec: &mut peniko::ColorStops) {
        vec.extend(
            self.colors
                .iter()
                .zip(self.stops.iter())
                .map(|(&color, &offset)| ColorStop {
                    offset,
                    color: to_vello_color(color).into(),
                }),
        );
    }
}

#[derive(Debug)]
enum RenderStyle {
    Fill,
    Stroke(Stroke),
}

#[derive(Debug)]
pub struct Paint {
    style: RenderStyle,
    brush: Brush,
    blend_mode: BlendMode,
}

impl Default for Paint {
    #[inline]
    fn default() -> Self {
        Self {
            style: RenderStyle::Fill,
            brush: Brush::Solid(Color::TRANSPARENT),
            blend_mode: Mix::Normal.into(),
        }
    }
}

impl renderer::Paint for Paint {
    type Gradient = Gradient;

    #[inline]
    fn set_style(&mut self, style: renderer::PaintStyle) {
        self.style = match style {
            renderer::PaintStyle::Stroke => RenderStyle::Stroke(Stroke::new(0.0)),
            renderer::PaintStyle::Fill => RenderStyle::Fill,
        }
    }

    #[inline]
    fn set_color(&mut self, color: renderer::Color) {
        self.brush = Brush::Solid(to_vello_color(color));
    }

    #[inline]
    fn set_thickness(&mut self, thickness: f32) {
        loop {
            if let RenderStyle::Stroke(stroke) = &mut self.style {
                stroke.width = thickness as f64;
                break;
            } else {
                self.style = RenderStyle::Stroke(Stroke::new(0.0));
            }
        }
    }

    #[inline]
    fn set_join(&mut self, join: renderer::StrokeJoin) {
        loop {
            if let RenderStyle::Stroke(stroke) = &mut self.style {
                stroke.join = match join {
                    renderer::StrokeJoin::Miter => Join::Miter,
                    renderer::StrokeJoin::Round => Join::Round,
                    renderer::StrokeJoin::Bevel => Join::Bevel,
                };
                break;
            } else {
                self.style = RenderStyle::Stroke(Stroke::new(0.0));
            }
        }
    }

    #[inline]
    fn set_cap(&mut self, cap: renderer::StrokeCap) {
        loop {
            if let RenderStyle::Stroke(stroke) = &mut self.style {
                stroke.start_cap = match cap {
                    renderer::StrokeCap::Butt => Cap::Butt,
                    renderer::StrokeCap::Round => Cap::Round,
                    renderer::StrokeCap::Square => Cap::Square,
                };
                stroke.end_cap = stroke.start_cap;
                break;
            } else {
                self.style = RenderStyle::Stroke(Stroke::new(0.0));
            }
        }
    }

    #[inline]
    fn set_blend_mode(&mut self, blend_mode: renderer::BlendMode) {
        self.blend_mode = to_vello_mix(blend_mode).into();
    }

    #[inline]
    fn set_gradient(&mut self, gradient: &Self::Gradient) {
        self.brush = Brush::Gradient(gradient.inner.clone());
    }

    #[inline]
    fn invalidate_stroke(&mut self) {}
}

#[derive(Debug)]
pub struct Gradient {
    inner: peniko::Gradient,
}

impl renderer::Gradient for Gradient {
    #[inline]
    fn new_linear(
        sx: f32,
        sy: f32,
        ex: f32,
        ey: f32,
        colors: &[renderer::Color],
        stops: &[f32],
    ) -> Self {
        let stops = SliceStops { colors, stops };
        Gradient {
            inner: peniko::Gradient::new_linear((sx as f64, sy as f64), (ex as f64, ey as f64))
                .with_stops(stops),
        }
    }

    #[inline]
    fn new_radial(
        cx: f32,
        cy: f32,
        radius: f32,
        colors: &[renderer::Color],
        stops: &[f32],
    ) -> Self {
        let stops = SliceStops { colors, stops };
        Gradient {
            inner: peniko::Gradient::new_radial((cx as f64, cy as f64), radius).with_stops(stops),
        }
    }
}

#[derive(Debug)]
pub struct Image {
    inner: peniko::ImageData,
}

impl renderer::Image for Image {
    fn decode(data: &[u8]) -> Option<Self> {
        let image = Reader::new(Cursor::new(data))
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()?
            .into_rgba8();
        let width = image.width();
        let height = image.height();

        Some(Image {
            inner: peniko::ImageData {
                data: image.into_raw().into(),
                format: ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width,
                height,
            },
        })
    }
}

pub struct Renderer {
    scene: Scene,
    transforms: Vec<Affine>,
    clips: Vec<usize>,
}

impl Renderer {
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn into_scene(self) -> Scene {
        self.scene
    }

    fn last_transform(&mut self) -> &mut Affine {
        self.transforms.last_mut().unwrap()
    }

    fn last_clip(&mut self) -> &mut usize {
        self.clips.last_mut().unwrap()
    }
}

impl Default for Renderer {
    #[inline]
    fn default() -> Self {
        Self {
            scene: Scene::new(),
            transforms: vec![Affine::IDENTITY],
            clips: vec![0],
        }
    }
}

impl renderer::Renderer for Renderer {
    type Buffer = Buffer;

    type Path = Path;

    type Paint = Paint;

    type Gradient = Gradient;

    type Image = Image;

    #[inline]
    fn state_push(&mut self) {
        let last_transform = *self.last_transform();

        self.transforms.push(last_transform);
        self.clips.push(0);
    }

    #[inline]
    fn state_pop(&mut self) {
        self.transforms.pop();
        for _ in 0..self.clips.pop().unwrap_or_default() {
            self.scene.pop_layer();
        }

        if self.transforms.is_empty() {
            self.transforms.push(Affine::IDENTITY);
            self.clips.push(0);
        }
    }

    #[inline]
    fn transform(&mut self, transform: &[f32; 6]) {
        let last_transform = self.last_transform();
        *last_transform *= Affine::new((*transform).map(Into::into));
    }

    #[inline]
    fn set_clip(&mut self, path: &Self::Path) {
        let transform = *self.last_transform();

        self.scene
            .push_clip_layer(path.fill, transform, &path.inner);

        // Rive can apply several masks between save/restore calls. Each mask
        // intersects the previous clips and must remain active until restore.
        *self.last_clip() += 1;
    }

    #[inline]
    fn draw_path(&mut self, path: &Self::Path, paint: &Self::Paint) {
        let transform = *self.last_transform();

        let builder = &mut self.scene;

        let skip_blending = paint.blend_mode == Mix::Normal.into();

        if !skip_blending {
            let mut bounds = path.inner.bounding_box();
            if let RenderStyle::Stroke(stroke) = &paint.style {
                // Include round/square caps and miter joins in the blend layer.
                let extent =
                    stroke.width.abs() * 0.5 * stroke.miter_limit.max(std::f64::consts::SQRT_2);
                bounds = bounds.inflate(extent, extent);
            }
            builder.push_layer(Fill::NonZero, paint.blend_mode, 1.0, transform, &bounds);
        }

        match &paint.style {
            RenderStyle::Fill => {
                builder.fill(path.fill, transform, &paint.brush, None, &path.inner)
            }
            RenderStyle::Stroke(stroke) => {
                builder.stroke(stroke, transform, &paint.brush, None, &path.inner)
            }
        }

        if !skip_blending {
            builder.pop_layer();
        }
    }

    #[inline]
    fn draw_image(&mut self, image: &Self::Image, blend_mode: renderer::BlendMode, opacity: f32) {
        let image = &image.inner;
        let mix: Mix = to_vello_mix(blend_mode);

        let transform = self.last_transform().pre_translate(Vec2::new(
            image.width as f64 * -0.5,
            image.height as f64 * -0.5,
        ));
        let rect = Rect::new(0.0, 0.0, image.width as f64, image.height as f64);

        let builder = &mut self.scene;

        let skip_blending = mix == Mix::Normal && opacity == 1.0;

        if !skip_blending {
            builder.push_layer(Fill::NonZero, mix, opacity, transform, &rect);
        }

        builder.draw_image(image, transform);

        if !skip_blending {
            builder.pop_layer();
        }
    }

    #[inline]
    fn draw_image_mesh(
        &mut self,
        image: &Self::Image,
        vertices: &Self::Buffer,
        uvs: &Self::Buffer,
        indices: &Self::Buffer,
        blend_mode: renderer::BlendMode,
        opacity: f32,
    ) {
        let image = &image.inner;
        let vertices = vertices.as_f32_pairs();
        let uvs = uvs.as_f32_pairs();

        let mix: Mix = to_vello_mix(blend_mode);
        let skip_blending = mix == Mix::Normal && opacity == 1.0;
        let transform = *self.last_transform();

        if !skip_blending {
            let Some(first) = vertices.first() else {
                return;
            };
            let mut bounds = Rect::from_points(
                Point::new(first[0] as f64, first[1] as f64),
                Point::new(first[0] as f64, first[1] as f64),
            );
            for vertex in vertices.iter().skip(1) {
                bounds = bounds.union_pt(Point::new(vertex[0] as f64, vertex[1] as f64));
            }
            // Triangle expansion below can extend outside the original mesh.
            bounds = bounds.inflate(bounds.width() * 0.03, bounds.height() * 0.03);
            self.scene
                .push_layer(Fill::NonZero, mix, opacity, transform, &bounds);
        }

        for triangle_indices in indices.as_u16s().chunks_exact(3) {
            let points = [
                vertices[triangle_indices[0] as usize],
                vertices[triangle_indices[1] as usize],
                vertices[triangle_indices[2] as usize],
            ];
            let uvs = [
                uvs[triangle_indices[0] as usize],
                uvs[triangle_indices[1] as usize],
                uvs[triangle_indices[2] as usize],
            ];

            let center = Point::new(
                ((points[0][0] + points[1][0] + points[2][0]) / 3.0) as f64,
                ((points[0][1] + points[1][1] + points[2][1]) / 3.0) as f64,
            );

            let path = triangle_path(points.map(|v| Point::new(v[0] as f64, v[1] as f64)));

            let transform = transform.pre_scale_from_origin(1.03, center);
            let brush_transform =
                util::map_uvs_to_triangle(&points, &uvs, image.width, image.height);

            let builder = &mut self.scene;

            builder.fill(
                Fill::NonZero,
                transform,
                BrushRef::Image(image.into()),
                Some(brush_transform),
                &path,
            );
        }

        // Composite the entire mesh once. Per-triangle opacity compounds at
        // the overlap used to hide antialiasing seams between triangles.
        if !skip_blending {
            self.scene.pop_layer();
        }
    }
}

impl fmt::Debug for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Renderer")
            .field("transforms", &self.transforms)
            .field("clips", &self.clips)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{Buffer as _, Paint as _, Renderer as _};

    fn rectangle() -> Path {
        Path {
            inner: Rect::new(0.0, 0.0, 10.0, 10.0).to_path(0.1),
            fill: Fill::NonZero,
        }
    }

    #[test]
    fn successive_clips_intersect_and_restore_together() {
        let mut renderer = Renderer::default();
        renderer.set_clip(&rectangle());
        renderer.state_push();
        renderer.set_clip(&rectangle());
        renderer.set_clip(&rectangle());
        assert_eq!(renderer.scene().encoding().n_open_clips, 3);
        renderer.state_pop();
        assert_eq!(renderer.scene().encoding().n_open_clips, 1);
        renderer.state_pop();
        assert_eq!(renderer.scene().encoding().n_open_clips, 0);
    }

    #[test]
    fn empty_buffers_have_valid_typed_views() {
        let buffer = Buffer::new(renderer::BufferType::Vertex, renderer::BufferFlags::NONE, 0);
        assert!(buffer.as_f32_pairs().is_empty());
        assert!(buffer.as_u16s().is_empty());
    }

    #[test]
    fn index_buffers_do_not_expose_alignment_padding() {
        let mut buffer = Buffer::new(renderer::BufferType::Index, renderer::BufferFlags::NONE, 6);
        buffer
            .map()
            .copy_from_slice(bytemuck::cast_slice(&[3_u16, 4, 5]));
        assert_eq!(buffer.as_u16s(), &[3, 4, 5]);
    }

    #[test]
    fn blended_stroke_clip_contains_the_stroke_width() {
        let mut inner = BezPath::new();
        inner.move_to((0.0, 0.0));
        inner.line_to((10.0, 0.0));
        let path = Path {
            inner,
            fill: Fill::NonZero,
        };
        let mut paint = Paint::default();
        paint.set_thickness(4.0);
        paint.set_blend_mode(renderer::BlendMode::Multiply);
        let mut renderer = Renderer::default();
        renderer.draw_path(&path, &paint);
        // The first encoded path bounds the blend layer, before the stroke itself.
        let clip_points: &[[f32; 2]] =
            bytemuck::cast_slice(&renderer.scene().encoding().path_data[..10]);
        assert!(clip_points.iter().any(|point| point[1] <= -2.0));
        assert!(clip_points.iter().any(|point| point[1] >= 2.0));
    }

    #[test]
    fn mesh_opacity_is_applied_once_to_the_whole_mesh() {
        fn buffer<T: bytemuck::Pod>(data: &[T], kind: renderer::BufferType) -> Buffer {
            let bytes = bytemuck::cast_slice(data);
            let mut buffer = Buffer::new(kind, renderer::BufferFlags::NONE, bytes.len());
            buffer.map().copy_from_slice(bytes);
            buffer
        }
        let image = Image {
            inner: peniko::ImageData {
                data: vec![255; 16].into(),
                format: ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width: 2,
                height: 2,
            },
        };
        let vertices = buffer(
            &[[0.0_f32, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            renderer::BufferType::Vertex,
        );
        let uvs = buffer(
            &[[0.0_f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            renderer::BufferType::Vertex,
        );
        let indices = buffer(&[0_u16, 1, 2, 2, 3, 0], renderer::BufferType::Index);
        let mut renderer = Renderer::default();
        renderer.draw_image_mesh(
            &image,
            &vertices,
            &uvs,
            &indices,
            renderer::BlendMode::SrcOver,
            0.5,
        );
        // One begin and one end: adjacent triangles must not each composite at 50%.
        assert_eq!(renderer.scene().encoding().n_clips, 2);
        assert_eq!(renderer.scene().encoding().n_open_clips, 0);
    }
}
