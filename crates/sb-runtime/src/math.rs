//! Matrix math for the host contract: GL-style (forward Z, NDC `[-1, 1]`) projections,
//! Minecraft's camera rotation, and the Iris shadow and celestial constructions.
//!
//! Everything is computed in `f64` and converted to the column-major `f32` layout that
//! std140 uniform blocks and [`sb_expr::Value::Mat4`] use only when uploading. This module
//! is the reference the Java mod mirrors, so every function documents the Iris/Minecraft
//! code it follows.

/// A 4x4 matrix in column-major order: `cols[c][r]` is row `r` of column `c`
/// (GLSL `m[c][r]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// The four columns.
    pub cols: [[f64; 4]; 4],
}

/// A 3-component vector.
pub type Vec3 = [f64; 3];
/// A 4-component vector.
pub type Vec4 = [f64; 4];

impl Default for Mat4 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat4 {
    /// The identity matrix.
    pub const IDENTITY: Mat4 = Mat4 {
        cols: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
    };

    /// The zero matrix.
    pub const ZERO: Mat4 = Mat4 { cols: [[0.0; 4]; 4] };

    /// Build from four columns.
    pub const fn from_cols(c0: Vec4, c1: Vec4, c2: Vec4, c3: Vec4) -> Self {
        Mat4 { cols: [c0, c1, c2, c3] }
    }

    /// Element at row `r`, column `c`.
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.cols[c][r]
    }

    /// Matrix product `self * rhs`.
    #[must_use]
    pub fn mul(&self, rhs: &Mat4) -> Mat4 {
        let mut out = Mat4::ZERO;
        for c in 0..4 {
            for r in 0..4 {
                out.cols[c][r] = (0..4).map(|k| self.cols[k][r] * rhs.cols[c][k]).sum();
            }
        }
        out
    }

    /// `self * v`.
    pub fn transform(&self, v: Vec4) -> Vec4 {
        let mut out = [0.0; 4];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|k| self.cols[k][r] * v[k]).sum();
        }
        out
    }

    /// `self * vec4(p, 1)` followed by the perspective divide.
    pub fn project_point(&self, p: Vec3) -> Vec3 {
        let v = self.transform([p[0], p[1], p[2], 1.0]);
        let w = if v[3].abs() < 1e-300 { 1.0 } else { v[3] };
        [v[0] / w, v[1] / w, v[2] / w]
    }

    /// The transpose.
    #[must_use]
    pub fn transpose(&self) -> Mat4 {
        let mut out = Mat4::ZERO;
        for c in 0..4 {
            for r in 0..4 {
                out.cols[c][r] = self.cols[r][c];
            }
        }
        out
    }

    /// The inverse, or `None` if the matrix is singular (or not finite).
    pub fn inverse(&self) -> Option<Mat4> {
        // Gauss-Jordan elimination with partial pivoting on a row-major copy.
        let mut a = [[0.0f64; 8]; 4];
        for (r, row) in a.iter_mut().enumerate() {
            for (c, col) in self.cols.iter().enumerate() {
                row[c] = col[r];
            }
            row[4 + r] = 1.0;
        }
        for col in 0..4 {
            let pivot = (col..4).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
            if !a[pivot][col].is_finite() || a[pivot][col].abs() < 1e-300 {
                return None;
            }
            a.swap(col, pivot);
            let p = a[col][col];
            for v in a[col].iter_mut() {
                *v /= p;
            }
            for r in 0..4 {
                if r != col {
                    let f = a[r][col];
                    if f != 0.0 {
                        let pivot_row = a[col];
                        for (v, pv) in a[r].iter_mut().zip(pivot_row) {
                            *v -= f * pv;
                        }
                    }
                }
            }
        }
        let mut out = Mat4::ZERO;
        for (r, row) in a.iter().enumerate() {
            for c in 0..4 {
                out.cols[c][r] = row[4 + c];
            }
        }
        out.cols.iter().flatten().all(|v| v.is_finite()).then_some(out)
    }

    /// The inverse, or the identity for a singular matrix (what a host uploads rather
    /// than NaNs).
    #[must_use]
    pub fn inverse_or_identity(&self) -> Mat4 {
        self.inverse().unwrap_or(Mat4::IDENTITY)
    }

    /// Translation by `(x, y, z)`.
    pub fn translation(x: f64, y: f64, z: f64) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.cols[3] = [x, y, z, 1.0];
        m
    }

    /// Non-uniform scale.
    pub fn scale(x: f64, y: f64, z: f64) -> Mat4 {
        Mat4::from_cols([x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0], [0.0, 0.0, 0.0, 1.0])
    }

    /// Right-handed rotation about +X by `deg` degrees (JOML `Axis.XP.rotationDegrees`).
    pub fn rotation_x(deg: f64) -> Mat4 {
        let (s, c) = deg.to_radians().sin_cos();
        Mat4::from_cols([1.0, 0.0, 0.0, 0.0], [0.0, c, s, 0.0], [0.0, -s, c, 0.0], [0.0, 0.0, 0.0, 1.0])
    }

    /// Right-handed rotation about +Y by `deg` degrees.
    pub fn rotation_y(deg: f64) -> Mat4 {
        let (s, c) = deg.to_radians().sin_cos();
        Mat4::from_cols([c, 0.0, -s, 0.0], [0.0, 1.0, 0.0, 0.0], [s, 0.0, c, 0.0], [0.0, 0.0, 0.0, 1.0])
    }

    /// Right-handed rotation about +Z by `deg` degrees.
    pub fn rotation_z(deg: f64) -> Mat4 {
        let (s, c) = deg.to_radians().sin_cos();
        Mat4::from_cols([c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0])
    }

    /// Column-major `f32` elements (`out[c * 4 + r]`), the std140 / `Value::Mat4` order.
    pub fn to_cols_f32(&self) -> [f32; 16] {
        let mut out = [0.0f32; 16];
        for c in 0..4 {
            for r in 0..4 {
                out[c * 4 + r] = self.cols[c][r] as f32;
            }
        }
        out
    }

    /// Whether every element differs from `other` by at most `eps`.
    pub fn approx_eq(&self, other: &Mat4, eps: f64) -> bool {
        self.cols.iter().flatten().zip(other.cols.iter().flatten()).all(|(a, b)| (a - b).abs() <= eps)
    }
}

/// `a * b` (Rust has no `Mul` impl here to keep the order explicit at call sites).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    a.mul(b)
}

/// GL `gluPerspective`: vertical field of view in degrees, NDC z in `[-1, 1]`
/// (near → -1, far → +1), looking down -Z.
pub fn perspective_gl(fov_y_deg: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let f = 1.0 / (fov_y_deg.to_radians() * 0.5).tan();
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    Mat4::from_cols(
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (far + near) / (near - far), -1.0],
        [0.0, 0.0, 2.0 * far * near / (near - far), 0.0],
    )
}

/// GL `glOrtho`.
pub fn ortho_gl(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Mat4 {
    Mat4::from_cols(
        [2.0 / (right - left), 0.0, 0.0, 0.0],
        [0.0, 2.0 / (top - bottom), 0.0, 0.0],
        [0.0, 0.0, -2.0 / (far - near), 0.0],
        [-(right + left) / (right - left), -(top + bottom) / (top - bottom), -(far + near) / (far - near), 1.0],
    )
}

/// Minecraft's camera rotation (`gbufferModelView`): `Rx(pitch) * Ry(yaw + 180)`, as in
/// `GameRenderer.renderLevel`. Yaw 0 faces south (+Z), yaw 90 faces west (-X); positive
/// pitch looks down. Positions are camera-relative, so there is no translation.
pub fn mc_view_rotation(yaw_deg: f64, pitch_deg: f64) -> Mat4 {
    Mat4::rotation_x(pitch_deg).mul(&Mat4::rotation_y(yaw_deg + 180.0))
}

/// World-space look direction for a Minecraft yaw/pitch (unit length).
pub fn mc_look_vector(yaw_deg: f64, pitch_deg: f64) -> Vec3 {
    let (sy, cy) = yaw_deg.to_radians().sin_cos();
    let (sp, cp) = pitch_deg.to_radians().sin_cos();
    [-sy * cp, -sp, cy * cp]
}

/// The Iris shadow matrices (`net.irisshaders.iris.shadows.ShadowMatrices`, Iris 1.11 for
/// Minecraft 26.3, GL depth convention).
pub mod shadow {
    use super::{Mat4, Vec3};

    /// Iris `ShadowMatrices.NEAR`: the default `shadowNearPlane`. Negative, because the
    /// 26.x shadow model-view no longer moves the light 100 blocks away from the camera.
    pub const NEAR: f64 = -100.05;
    /// Iris `ShadowMatrices.FAR`: the default `shadowFarPlane`.
    pub const FAR: f64 = 156.0;

    /// `createOrthoMatrix(halfPlaneLength, nearPlane, farPlane)`: JOML
    /// `setOrthoSymmetric(2h, 2h, near, far, zZeroToOne = false)`, i.e.
    /// `glOrtho(-h, h, -h, h, near, far)`.
    pub fn ortho(half_plane: f64, near: f64, far: f64) -> Mat4 {
        let h = if half_plane.abs() > 1e-9 { half_plane } else { 1.0 };
        let (near, far) = if (far - near).abs() > 1e-9 { (near, far) } else { (NEAR, FAR) };
        Mat4::from_cols(
            [1.0 / h, 0.0, 0.0, 0.0],
            [0.0, 1.0 / h, 0.0, 0.0],
            [0.0, 0.0, 2.0 / (near - far), 0.0],
            [0.0, 0.0, (far + near) / (near - far), 1.0],
        )
    }

    /// `createPerspectiveMatrix(fov)` (degrees) with the fixed [`NEAR`]/[`FAR`] planes,
    /// including Iris' `m33 = 1` quirk.
    pub fn perspective(fov_deg: f64) -> Mat4 {
        let y_scale = 1.0 / (fov_deg.to_radians() * 0.5).tan();
        Mat4::from_cols(
            [y_scale, 0.0, 0.0, 0.0],
            [0.0, y_scale, 0.0, 0.0],
            [0.0, 0.0, (FAR + NEAR) / (NEAR - FAR), -1.0],
            [0.0, 0.0, 2.0 * FAR * NEAR / (NEAR - FAR), 1.0],
        )
    }

    /// `createBaselineModelViewMatrix`: `Rx(90) * Rz(-360 * skyAngle) * Rx(sunPathRotation)`,
    /// where `skyAngle` is derived from the shadow angle.
    pub fn baseline_model_view(shadow_angle: f64, sun_path_rotation_deg: f64) -> Mat4 {
        let sky_angle = if shadow_angle < 0.25 { shadow_angle + 0.75 } else { shadow_angle - 0.25 };
        Mat4::rotation_x(90.0)
            .mul(&Mat4::rotation_z(sky_angle * -360.0))
            .mul(&Mat4::rotation_x(sun_path_rotation_deg))
    }

    /// The grid-snapping offset of `snapModelViewToGrid`, reproducing Java's float `%`
    /// (the sign follows the dividend, so negative coordinates yield negative offsets) and
    /// the `(float)` casts. `None` when `interval` is zero (no snapping).
    pub fn snap_offset(interval: f64, camera: Vec3) -> Option<Vec3> {
        let interval = interval as f32;
        if interval.abs() == 0.0 || !interval.is_finite() {
            return None;
        }
        let half = interval / 2.0;
        let off = |c: f64| f64::from((c as f32) % interval - half);
        Some([off(camera[0]), off(camera[1]), off(camera[2])])
    }

    /// `createModelViewMatrix`: the baseline matrix, translated by [`snap_offset`].
    pub fn model_view(shadow_angle: f64, sun_path_rotation_deg: f64, interval: f64, camera: Vec3) -> Mat4 {
        let base = baseline_model_view(shadow_angle, sun_path_rotation_deg);
        match snap_offset(interval, camera) {
            Some([x, y, z]) => base.mul(&Mat4::translation(x, y, z)),
            None => base,
        }
    }

    /// The near/far planes Iris uses for the ortho projection: `-1` means "DH render
    /// distance" (`-dh * 16` / `dh * 16`, with `dh_render_distance_chunks`).
    pub fn planes(near: f64, far: f64, dh_render_distance_chunks: f64) -> (f64, f64) {
        let near = if (near + 1.0).abs() < 1e-6 { -dh_render_distance_chunks * 16.0 } else { near };
        let far = if (far + 1.0).abs() < 1e-6 { dh_render_distance_chunks * 16.0 } else { far };
        (near, far)
    }
}

/// Sun/moon angles and positions (Minecraft `DimensionType.timeOfDay` and Iris
/// `CelestialUniforms`).
pub mod celestial {
    use super::{Mat4, Vec3};

    /// Minecraft's sky angle for a world time in ticks (0 = noon, 0.5 = midnight).
    pub fn sky_angle(world_time: i64) -> f64 {
        let d = (world_time.rem_euclid(24000) as f64 / 24000.0 - 0.25).rem_euclid(1.0);
        let e = 0.5 - (d * std::f64::consts::PI).cos() / 2.0;
        (d * 2.0 + e) / 3.0
    }

    /// Iris `sunAngle`: 0 at sunrise, 0.25 at noon, 0.5 at sunset.
    pub fn sun_angle(sky_angle: f64) -> f64 {
        if sky_angle < 0.75 { sky_angle + 0.25 } else { sky_angle - 0.75 }
    }

    /// Whether the sun is up (Iris `isDay`: the sun angle is below 180 degrees).
    pub fn is_day(sun_angle: f64) -> bool {
        sun_angle < 0.5
    }

    /// Iris `shadowAngle`: the sun angle by day, the moon angle (`sunAngle - 0.5`) by night.
    pub fn shadow_angle(sun_angle: f64) -> f64 {
        if is_day(sun_angle) { sun_angle } else { sun_angle - 0.5 }
    }

    /// Iris `getCelestialPosition(y)`: `gbufferModelView * Ry(-90) * Rz(sunPathRotation) *
    /// Rx(skyAngle * 360) * (0, y, 0, 0)`, in view space.
    pub fn position(model_view: &Mat4, sky_angle: f64, sun_path_rotation_deg: f64, y: f64) -> Vec3 {
        let m = model_view
            .mul(&Mat4::rotation_y(-90.0))
            .mul(&Mat4::rotation_z(sun_path_rotation_deg))
            .mul(&Mat4::rotation_x(sky_angle * 360.0));
        let v = m.transform([0.0, y, 0.0, 0.0]);
        [v[0], v[1], v[2]]
    }

    /// Iris `upPosition`: `gbufferModelView * Ry(-90) * (0, 100, 0, 0)`.
    pub fn up_position(model_view: &Mat4) -> Vec3 {
        let v = model_view.mul(&Mat4::rotation_y(-90.0)).transform([0.0, 100.0, 0.0, 0.0]);
        [v[0], v[1], v[2]]
    }

    /// The celestial model-view used to draw the sun and moon quads (vanilla
    /// `LevelRenderer.renderSky`): `gbufferModelView * Ry(-90) * Rz(sunPathRotation) *
    /// Rx(skyAngle * 360)`.
    pub fn sky_model_view(model_view: &Mat4, sky_angle: f64, sun_path_rotation_deg: f64) -> Mat4 {
        model_view
            .mul(&Mat4::rotation_y(-90.0))
            .mul(&Mat4::rotation_z(sun_path_rotation_deg))
            .mul(&Mat4::rotation_x(sky_angle * 360.0))
    }
}

/// Vector helpers.
pub mod vec {
    use super::Vec3;

    /// Dot product.
    pub fn dot(a: Vec3, b: Vec3) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// Length.
    pub fn length(a: Vec3) -> f64 {
        dot(a, a).sqrt()
    }

    /// Unit vector (zero stays zero).
    pub fn normalize(a: Vec3) -> Vec3 {
        let l = length(a);
        if l > 1e-300 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0; 3] }
    }

    /// `a - b`.
    pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    /// `f32` copy.
    pub fn to_f32(a: Vec3) -> [f32; 3] {
        [a[0] as f32, a[1] as f32, a[2] as f32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn inverse_roundtrip() {
        let m = perspective_gl(70.0, 16.0 / 9.0, 0.05, 256.0)
            .mul(&mc_view_rotation(37.0, 12.0))
            .mul(&Mat4::translation(1.0, -2.0, 3.5));
        let inv = m.inverse().expect("invertible");
        assert!(m.mul(&inv).approx_eq(&Mat4::IDENTITY, 1e-9));
        assert!(inv.mul(&m).approx_eq(&Mat4::IDENTITY, 1e-9));
        assert!(Mat4::ZERO.inverse().is_none());
        assert_eq!(Mat4::ZERO.inverse_or_identity(), Mat4::IDENTITY);
        let nan = Mat4::from_cols([f64::NAN; 4], [0.0; 4], [0.0; 4], [0.0; 4]);
        assert!(nan.inverse().is_none());
    }

    #[test]
    fn perspective_maps_near_and_far_to_gl_ndc() {
        let p = perspective_gl(70.0, 2.0, 0.05, 128.0);
        assert!(close(p.project_point([0.0, 0.0, -0.05])[2], -1.0, 1e-9));
        assert!(close(p.project_point([0.0, 0.0, -128.0])[2], 1.0, 1e-9));
        // Vertical fov: a point at the top edge of the frustum maps to y = 1.
        let t = (35.0f64).to_radians().tan();
        assert!(close(p.project_point([0.0, t * 10.0, -10.0])[1], 1.0, 1e-9));
        assert!(close(p.project_point([2.0 * t * 10.0, 0.0, -10.0])[0], 1.0, 1e-9));
        // Inverse brings NDC back to view space.
        let inv = p.inverse().unwrap();
        let v = inv.project_point([0.0, 0.0, -1.0]);
        assert!(close(v[2], -0.05, 1e-9));
    }

    #[test]
    fn ortho_matches_gl() {
        let o = ortho_gl(-2.0, 2.0, -1.0, 1.0, 0.5, 10.0);
        assert!(close(o.project_point([2.0, 1.0, -0.5])[0], 1.0, EPS));
        assert!(close(o.project_point([2.0, 1.0, -0.5])[1], 1.0, EPS));
        assert!(close(o.project_point([0.0, 0.0, -0.5])[2], -1.0, EPS));
        assert!(close(o.project_point([0.0, 0.0, -10.0])[2], 1.0, EPS));
        // Iris' ortho is glOrtho(-h, h, -h, h, n, f).
        assert!(shadow::ortho(160.0, 0.05, 256.0).approx_eq(&ortho_gl(-160.0, 160.0, -160.0, 160.0, 0.05, 256.0), 1e-12));
    }

    #[test]
    fn mc_camera_conventions() {
        for (yaw, pitch) in [(0.0, 0.0), (90.0, 0.0), (-135.0, 30.0), (200.0, -60.0)] {
            let view = mc_view_rotation(yaw, pitch);
            let f = mc_look_vector(yaw, pitch);
            let v = view.transform([f[0], f[1], f[2], 0.0]);
            assert!(close(v[0], 0.0, 1e-9) && close(v[1], 0.0, 1e-9) && close(v[2], -1.0, 1e-9), "{yaw} {pitch}: {v:?}");
        }
        // Yaw 0 faces south (+Z), yaw 90 faces west (-X), positive pitch looks down.
        let f = mc_look_vector(0.0, 0.0);
        assert!(close(f[2], 1.0, EPS));
        let f = mc_look_vector(90.0, 0.0);
        assert!(close(f[0], -1.0, EPS));
        assert!(mc_look_vector(0.0, 45.0)[1] < 0.0);
        // Rotation only: inverse is the transpose.
        let view = mc_view_rotation(33.0, 21.0);
        assert!(view.inverse().unwrap().approx_eq(&view.transpose(), 1e-12));
    }

    #[test]
    fn celestial_angles_follow_minecraft_and_iris() {
        // Noon (6000 ticks): sky angle 0, sun angle 0.25, sun straight up.
        let noon = celestial::sky_angle(6000);
        assert!(close(noon, 0.0, 1e-12));
        assert!(close(celestial::sun_angle(noon), 0.25, 1e-12));
        let view = Mat4::IDENTITY;
        let sun = celestial::position(&view, noon, 0.0, 100.0);
        assert!(close(sun[1], 100.0, 1e-9) && close(sun[0], 0.0, 1e-9));
        let moon = celestial::position(&view, noon, 0.0, -100.0);
        assert!(close(moon[1], -100.0, 1e-9));
        // Midnight (18000): sky angle 0.5, the moon casts shadows.
        let midnight = celestial::sky_angle(18000);
        assert!(close(midnight, 0.5, 1e-12));
        let sa = celestial::sun_angle(midnight);
        assert!(close(sa, 0.75, 1e-12));
        assert!(!celestial::is_day(sa));
        assert!(close(celestial::shadow_angle(sa), 0.25, 1e-12));
        // Morning: the sun rises in the east (+X).
        let morning = celestial::sky_angle(1000);
        let sun = celestial::position(&view, morning, 0.0, 100.0);
        assert!(sun[0] > 50.0 && sun[1] > 0.0, "{sun:?}");
        // Up position is the view-space up vector scaled by 100.
        let up = celestial::up_position(&mc_view_rotation(10.0, 30.0));
        assert!(close(vec::length(up), 100.0, 1e-9));
    }

    #[test]
    fn iris_shadow_model_view() {
        // At noon (shadow angle 0.25) without path rotation the shadow camera looks
        // straight down: points below the camera have negative view z.
        let mv = shadow::baseline_model_view(0.25, 0.0);
        let below = mv.transform([0.0, -50.0, 0.0, 1.0]);
        assert!(close(below[0], 0.0, 1e-9) && close(below[1], 0.0, 1e-9));
        assert!(close(below[2], -50.0, 1e-9), "{below:?}");
        let x = mv.transform([1.0, 0.0, 0.0, 0.0]);
        assert!(close(x[0], 1.0, 1e-9));
        // sunPathRotation tilts the light around X; the result stays a rotation.
        let tilted = shadow::baseline_model_view(0.25, 30.0);
        assert!(!tilted.approx_eq(&mv, 1e-6));
        assert!(tilted.inverse().unwrap().approx_eq(&tilted.transpose(), 1e-9));
    }

    #[test]
    fn iris_shadow_model_view_at_dawn_matches_iris_test_vector() {
        // ShadowMatrices.Tests "model view at dawn". The rotation part is unchanged in
        // Iris 26.3; the translation lost the old `-100` z offset.
        let mv = shadow::model_view(0.034_517_77, 0.0, 2.0, [0.646_045_982_837_677, 82.532_745_361_328_12, -514.026_428_222_656_2]);
        let expected = Mat4::from_cols(
            [0.215_450_406, 5.820_481_5e-8, 0.976_514_697, 0.0],
            [-0.976_514_746, 1.284_184_5e-8, 0.215_450_391, 0.0],
            [0.0, -0.999_999_94, 5.960_464_5e-8, 0.0],
            [0.380_021_512, 1.026_428_103, -100.446_311_95 + 100.0, 1.0],
        );
        assert!(mv.approx_eq(&expected, 5e-4), "{mv:?}");
    }

    #[test]
    fn iris_shadow_projections() {
        // ShadowMatrices.Tests "ortho projection hpl=32" (explicit planes 0.05 / 256).
        let o = shadow::ortho(32.0, 0.05, 256.0);
        assert!(close(o.at(0, 0), 0.03125, 1e-9));
        assert!(close(o.at(2, 2), -0.007_814_026_437_699_795, 1e-9));
        assert!(close(o.at(2, 3), -1.000_390_648_841_858, 1e-6));
        assert!(o.approx_eq(&ortho_gl(-32.0, 32.0, -32.0, 32.0, 0.05, 256.0), 1e-12));
        // Default planes straddle the camera: geometry 100 blocks towards the light
        // is still inside the clip volume.
        let d = shadow::ortho(160.0, shadow::NEAR, shadow::FAR);
        assert!(d.project_point([0.0, 0.0, 99.0])[2] > -1.0);
        assert!(d.project_point([0.0, 0.0, -150.0])[2] < 1.0);
        assert_eq!(shadow::planes(-1.0, -1.0, 32.0), (-512.0, 512.0));
        assert_eq!(shadow::planes(0.5, 300.0, 32.0), (0.5, 300.0));
    }

    #[test]
    fn iris_shadow_grid_snapping() {
        // Positive coordinates: offset = c % interval - interval / 2.
        let o = shadow::snap_offset(2.0, [5.5, 64.25, 3.0]).unwrap();
        assert!(close(o[0], 1.5 - 1.0, 1e-6));
        assert!(close(o[1], 0.25 - 1.0, 1e-6));
        assert!(close(o[2], 1.0 - 1.0, 1e-6));
        // Java's % keeps the sign of the dividend: -2.5 % 2 = -0.5.
        let o = shadow::snap_offset(2.0, [-2.5, 0.0, 0.0]).unwrap();
        assert!(close(o[0], -0.5 - 1.0, 1e-6));
        assert!(shadow::snap_offset(0.0, [1.0, 2.0, 3.0]).is_none());
        // The snapped matrix is the baseline translated by the offset.
        let snapped = shadow::model_view(0.3, 10.0, 2.0, [5.5, 64.25, 3.0]);
        let base = shadow::baseline_model_view(0.3, 10.0);
        let p = snapped.transform([0.0, 0.0, 0.0, 1.0]);
        let q = base.transform([0.5, -0.75, 0.0, 1.0]);
        for i in 0..4 {
            assert!(close(p[i], q[i], 1e-5));
        }
        // Moving the camera by a whole interval does not move the grid.
        let a = shadow::model_view(0.3, 0.0, 2.0, [5.5, 64.0, 3.0]);
        let b = shadow::model_view(0.3, 0.0, 2.0, [7.5, 66.0, 5.0]);
        assert!(a.approx_eq(&b, 1e-5));
    }

    #[test]
    fn shadow_perspective_uses_iris_planes() {
        let p = shadow::perspective(90.0);
        assert!(close(p.at(0, 0), 1.0, 1e-12));
        assert!(close(p.at(2, 2), (shadow::FAR + shadow::NEAR) / (shadow::NEAR - shadow::FAR), 1e-12));
        assert!(close(p.at(3, 2), -1.0, 1e-12));
        // Iris puts 1 in m33 (as written in ShadowMatrices); keep that quirk.
        assert!(close(p.at(3, 3), 1.0, 1e-12));
    }
}
