//! What a frame is allowed to cost.
//!
//! Every renderer feature in this game is a trade, and the trades are not the
//! same on every machine: ray queries exist on some GPUs and not others, a
//! shadow map that is free at 1080p is not free at 4K, and the geometry budget
//! that holds sixty frames a second on a desktop will not hold on a laptop.
//!
//! So nothing here is switched on directly. A [`QualityPreset`] is a single
//! choice a player makes, and it resolves to a [`GraphicsSettings`] — a flat
//! block of concrete numbers that the rest of `render` reads. Two consequences
//! fall out of that shape and both are deliberate:
//!
//! * **The preset is a request, not a promise.** [`GraphicsSettings::downgrade`]
//!   takes what the GPU actually reports and walks the settings back to
//!   something it can run. Asking for raytracing on hardware without ray
//!   queries gets you screen-space reflections, not a crash and not a black
//!   screen.
//! * **It is testable without a GPU.** The preset table and the downgrade rules
//!   are pure functions over plain data, so the interesting failure — a preset
//!   that quietly asks for something the tier below it does not — is a unit
//!   test rather than something you find in a screenshot three weeks later.
//!
//! The types are our own rather than Bevy's on purpose: `GraphicsSettings`
//! lives in `GameConfig`, which is serialised into save files, and a renderer
//! enum from a dependency is not something to write into a file on disk.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

/// The one knob a player turns.
///
/// Ordered, and the ordering is load-bearing: every rule in this module that
/// says "at least High" is a comparison, and the unit tests walk the tiers in
/// order to check that nothing gets cheaper as you go up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum QualityPreset {
    /// Integrated graphics. No screen-space work beyond ambient occlusion.
    Low,
    /// The floor for the game looking like itself: contact shadows and a real
    /// shadow distance.
    Medium,
    /// Where the city is meant to be seen. Deferred, reflections, volumetrics.
    #[default]
    High,
    /// Raytraced lighting where the hardware has it, and upscaling to pay for it.
    Ultra,
    /// Not a playable tier. Everything on, frame rate irrelevant — this is what
    /// `--screenshot` uses when the point is the picture rather than the game.
    Photo,
}

impl QualityPreset {
    pub const ALL: [Self; 5] = [
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Ultra,
        Self::Photo,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Ultra => "ultra",
            Self::Photo => "photo",
        }
    }

    /// Case-insensitive, for `--quality` and for the dev panel.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|p| p.name() == raw)
    }
}

/// How much of the horizon-based ambient occlusion to buy.
///
/// Mirrors Bevy's own quality levels rather than wrapping them, so this can be
/// serialised and so `render` owns the mapping in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AoQuality {
    Low,
    Medium,
    High,
    Ultra,
}

/// How far the volumetric pass goes.
///
/// Fog alone is the cheap half — it hazes the air and costs one full-screen
/// march. Lights is what actually sells a night street, because it is what puts
/// a visible cone under a lamp and a shaft between two buildings, and it costs
/// per light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Volumetrics {
    Off,
    /// A fog volume over the city, lit by the sun only.
    Fog,
    /// Street lamps and headlights cast visible shafts too.
    FogAndLights,
}

/// How the final image is resolved.
///
/// Not a quality dial so much as a choice of which temporal accumulator runs:
/// exactly one of these owns the jitter and the history buffer, so they are an
/// enum rather than a set of flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upscaling {
    /// No temporal pass at all. Aliases, but has no history to smear.
    Off,
    /// Temporal anti-aliasing at native resolution.
    Taa,
    /// DLSS super resolution, rendering below native and reconstructing up.
    ///
    /// Reachable through the dev panel and through a save file, and selected by
    /// no preset: nothing in this codebase attaches Bevy's `Dlss` component
    /// yet, and an upscaler that is asked for and never attached is worse than
    /// one that is not offered — it takes TAA off on the way past. The day the
    /// component is attached in `render::sync_camera_stack`, Ultra can name it
    /// again and [`GraphicsSettings::downgrade`] stops rewriting it.
    Dlss,
}

/// The resolved settings. Everything in `render` reads this and nothing else.
///
/// `#[serde(default)]` on the whole block, and it is load-bearing rather than
/// tidy. This is written into `saves/options.ron` in full, and the loader's
/// answer to a file it cannot parse is to throw the whole thing away and start
/// again — so a *single* field added here without a default silently resets
/// the player's city, costume and keybindings the first time they run the new
/// build. That is not hypothetical: it happened between one commit and the
/// next when `contact_shadow_length` and `sharpening` arrived, and the warning
/// line it printed was the only sign. Per-field defaults would have to be
/// remembered every time; this cannot be forgotten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    /// What was asked for. Kept after downgrading so the UI can say "Ultra
    /// (raytracing unavailable)" rather than silently claiming to be High.
    pub requested: QualityPreset,
    /// Side length of the directional light's shadow map, in texels.
    pub shadow_map_size: usize,
    /// How far from the camera the sun still casts. Clamped against the
    /// streaming radius when it is applied — there is no point shadowing
    /// geometry that has not been spawned.
    pub shadow_distance: f32,
    pub cascades: usize,
    /// Percentage-closer soft shadows: the penumbra widens with distance from
    /// whatever is casting, instead of every edge being equally hard.
    pub soft_shadows: bool,
    /// Short screen-space rays that put a shadow back where the shadow map's
    /// texel is too coarse to have one — under a bollard, under a wheel.
    pub contact_shadows: bool,
    /// How far those rays reach, in metres. A separate number from the flag
    /// because the default — thirty centimetres — is shorter than the things
    /// it exists to plant: it reaches a third of a wheel and a fifth of a lamp
    /// column's base flare, so a car sat on the road with nothing under it.
    /// Cost scales with the step count, not with the length, so the longer ray
    /// is very nearly free.
    pub contact_shadow_length: f32,
    pub ssao: Option<AoQuality>,
    /// Screen-space reflections. Requires the deferred path.
    pub ssr: bool,
    pub volumetrics: Volumetrics,
    pub motion_blur: bool,
    pub depth_of_field: bool,
    /// The artefacts of a real lens: a vignette, and a trace of chromatic
    /// aberration at the edge of the frame. Cheap, and pure luxury — the two
    /// lowest tiers spend the same milliseconds on something load-bearing.
    pub lens: bool,
    /// Multiplies every LOD switching distance. Below one, detail is dropped
    /// closer to the camera; above one it is held further out.
    pub lod_scale: f32,
    /// Raytraced direct and indirect lighting, *on top of* SSAO and SSR — see
    /// [`QualityPreset::settings`] for why it no longer replaces them.
    pub raytracing: bool,
    pub upscaling: Upscaling,
    /// Contrast-adaptive sharpening strength, 0 for none.
    ///
    /// Paired with [`Self::upscaling`] rather than with the tier: every
    /// temporal resolve hands back a softer image than it was given, and a
    /// sharpen afterwards is what every renderer that ships TAA does about it.
    /// A tier with no temporal pass has nothing to put back and asks for zero.
    pub sharpening: f32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        QualityPreset::default().settings()
    }
}

impl QualityPreset {
    /// The table. One row per tier, and the only place these numbers live.
    pub fn settings(self) -> GraphicsSettings {
        match self {
            Self::Low => GraphicsSettings {
                requested: self,
                shadow_map_size: 2048,
                shadow_distance: 300.0,
                cascades: 2,
                soft_shadows: false,
                contact_shadows: false,
                contact_shadow_length: 0.0,
                ssao: Some(AoQuality::Low),
                ssr: false,
                volumetrics: Volumetrics::Off,
                motion_blur: false,
                depth_of_field: false,
                lens: false,
                lod_scale: 0.6,
                raytracing: false,
                // No history buffer at all: on the hardware this tier is for,
                // TAA's resolve costs more than the aliasing it removes.
                upscaling: Upscaling::Off,
                // And nothing softened the image, so there is nothing to
                // sharpen. CAS over an un-resolved frame only sharpens the
                // aliasing.
                sharpening: 0.0,
            },
            Self::Medium => GraphicsSettings {
                requested: self,
                shadow_map_size: 2048,
                shadow_distance: 500.0,
                cascades: 3,
                // On at Medium, and it is the cheapest thing on this row.
                // Percentage-closer soft shadows are a compiled-in bevy
                // feature already paid for, and this is the tier with three
                // cascades over five hundred metres — a coarse map with a
                // razor edge on it reads far worse than a coarse map with a
                // penumbra, because the penumbra is what hides the stair-step
                // along a shadow terminator.
                soft_shadows: true,
                contact_shadows: true,
                contact_shadow_length: 0.7,
                ssao: Some(AoQuality::Medium),
                // On at Medium too. `reflections()` caps the roughness window
                // below car paint, so the march only ever runs on standing
                // water, wet pavement and glass — and those are the only
                // surfaces in the city that can put a pixel above white in
                // daylight. The deferred path and the blue-noise texture are
                // paid for at every tier regardless.
                ssr: true,
                volumetrics: Volumetrics::Fog,
                motion_blur: false,
                depth_of_field: false,
                lens: false,
                lod_scale: 0.85,
                raytracing: false,
                upscaling: Upscaling::Taa,
                sharpening: 0.4,
            },
            Self::High => GraphicsSettings {
                requested: self,
                shadow_map_size: 4096,
                shadow_distance: 900.0,
                cascades: 4,
                soft_shadows: true,
                contact_shadows: true,
                contact_shadow_length: 1.1,
                ssao: Some(AoQuality::High),
                ssr: true,
                volumetrics: Volumetrics::FogAndLights,
                motion_blur: true,
                depth_of_field: false,
                lens: true,
                lod_scale: 1.0,
                raytracing: false,
                upscaling: Upscaling::Taa,
                // Below Bevy's 0.6 default on purpose: a city is high-frequency
                // to begin with — a facade is a grid of window reveals — and
                // 0.6 rings along every one of them.
                sharpening: 0.4,
            },
            Self::Ultra => GraphicsSettings {
                requested: self,
                shadow_map_size: 8192,
                shadow_distance: 1200.0,
                cascades: 4,
                soft_shadows: true,
                contact_shadows: true,
                contact_shadow_length: 1.1,
                // These used to be `None` and `false`, on the reasoning that
                // raytraced lighting computes its own occlusion and a second
                // screen-space estimate on top of it double-darkens corners.
                // The reasoning is sound and the premise was not: nothing in
                // this codebase ever attached `SolariLighting`, so
                // `raytracing: true` was a flag three places read and no pass
                // honoured. Under the default build `downgrade` put both back
                // and it never showed; anyone building `--features raytracing`
                // on capable hardware got an Ultra with no occlusion term, no
                // reflections and no raytracing — strictly flatter than High.
                //
                // So the flag now *adds* rather than replaces, and the day a
                // raytraced pass lands it is that pass's job to decide what to
                // switch off. A preset table that promises something no system
                // delivers is worse than a preset table that promises less.
                ssao: Some(AoQuality::Ultra),
                ssr: true,
                volumetrics: Volumetrics::FogAndLights,
                motion_blur: true,
                depth_of_field: true,
                lens: true,
                lod_scale: 1.3,
                raytracing: true,
                // Not `Dlss`, and that is a correction rather than a choice.
                // Selecting it made `render::sync_camera_stack` take TAA *off*
                // and put nothing on in its place — the DLSS component was
                // always going to be attached "in the raytracing pass once that
                // lands", and it never landed. Meanwhile `shadows` picks the
                // temporal shadow filter for Dlss and `volumetrics` jitters the
                // raymarch for it, both of which are only correct because
                // something resolves the per-frame variation. So on a machine
                // built `--features dlss` the top playable tier ran with no
                // anti-aliasing at all over a crawling shadow filter and a
                // crawling raymarch. A preset may not name an accumulator
                // nothing attaches; see [`Upscaling::Dlss`].
                upscaling: Upscaling::Taa,
                sharpening: 0.3,
            },
            Self::Photo => GraphicsSettings {
                requested: self,
                shadow_map_size: 8192,
                shadow_distance: 2000.0,
                cascades: 4,
                soft_shadows: true,
                contact_shadows: true,
                contact_shadow_length: 1.1,
                ssao: Some(AoQuality::Ultra),
                ssr: true,
                volumetrics: Volumetrics::FogAndLights,
                // Both are shutter effects, and a still has no shutter. They
                // would only smear the thing the shot exists to show.
                motion_blur: false,
                depth_of_field: true,
                lens: true,
                // Nothing is ever allowed to drop detail: a still is judged at
                // full size, and a switched LOD is the one artefact that cannot
                // be argued away afterwards.
                lod_scale: f32::INFINITY,
                raytracing: true,
                // A still can afford to accumulate honestly rather than
                // reconstruct from a lower resolution.
                upscaling: Upscaling::Taa,
                // Lighter still. A still is looked at closely, and the halo a
                // sharpen leaves along a high-contrast edge is exactly the sort
                // of artefact that cannot be argued away at full size.
                sharpening: 0.25,
            },
        }
    }
}

/// What the GPU turned out to support.
///
/// Filled in once, after the render device exists, by querying `wgpu` features
/// — see `render::quality::probe`. Kept as plain bools so the downgrade rules
/// stay unit-testable with no device in the room.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// `EXPERIMENTAL_RAY_QUERY` and the binding-array features Solari needs.
    pub raytracing: bool,
    /// NVIDIA DLSS, which also requires the `dlss` cargo feature to be built in.
    pub dlss: bool,
}

impl Capabilities {
    /// Everything available. The baseline the preset table is written against.
    pub fn all() -> Self {
        Self {
            raytracing: true,
            dlss: true,
        }
    }
}

impl GraphicsSettings {
    /// Walks the settings back to what this machine can actually run.
    ///
    /// The rule is that a missing capability falls back to the nearest thing
    /// that produces a comparable picture, not to nothing: without ray queries
    /// the lighting keeps the screen-space reflections *and* the ambient
    /// occlusion, because otherwise dropping raytracing would leave corners
    /// with no occlusion term at all and the scene would come out flatter than
    /// High. That used to be a repair — Ultra switched both off and this put
    /// them back — and is now merely a guarantee, because the preset table no
    /// longer switches them off in the first place.
    pub fn downgrade(mut self, caps: Capabilities) -> Self {
        if self.raytracing && !caps.raytracing {
            self.raytracing = false;
            self.ssr = true;
            self.ssao = self.ssao.or(Some(AoQuality::High));
        }
        // Unconditional, not `&& !caps.dlss`: the hardware half was never the
        // problem. Nothing attaches Bevy's `Dlss` component, so selecting it
        // removes TAA and inserts nothing — see [`Upscaling::Dlss`]. This
        // catches a setting arriving from a save file; `sync_camera_stack`
        // catches one arriving from the dev panel, because this runs once at
        // startup and the panel writes whenever a slider moves.
        if self.upscaling == Upscaling::Dlss {
            self.upscaling = Upscaling::Taa;
        }
        self
    }

    /// True when something on the camera consumes motion vectors.
    ///
    /// TAA, DLSS and motion blur all need the same prepass, and asking for it
    /// three times independently is how it ends up requested in one code path
    /// and forgotten in another.
    pub fn needs_motion_vectors(&self) -> bool {
        self.motion_blur || matches!(self.upscaling, Upscaling::Taa | Upscaling::Dlss)
    }

    /// The distance at which a mesh should drop to its next level of detail,
    /// given the distance the art was authored for.
    pub fn lod_distance(&self, base: f32) -> f32 {
        base * self.lod_scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_survives_a_round_trip_through_its_name() {
        for preset in QualityPreset::ALL {
            assert_eq!(QualityPreset::parse(preset.name()), Some(preset));
        }
        assert_eq!(QualityPreset::parse("  ULTRA "), Some(QualityPreset::Ultra));
        assert_eq!(QualityPreset::parse("cinematic"), None);
    }

    /// The point of an ordered tier list is that going up never takes something
    /// away. Anything that genuinely gets *smaller* higher up is a considered
    /// exception and is listed here by name rather than left to be rediscovered.
    #[test]
    fn nothing_gets_cheaper_as_the_tier_goes_up() {
        for pair in QualityPreset::ALL.windows(2) {
            let (lower, upper) = (pair[0].settings(), pair[1].settings());
            let tier = pair[1].name();

            assert!(
                upper.shadow_map_size >= lower.shadow_map_size,
                "{tier} shrank the shadow map"
            );
            assert!(
                upper.shadow_distance >= lower.shadow_distance,
                "{tier} pulled the shadow distance in"
            );
            assert!(upper.cascades >= lower.cascades, "{tier} dropped a cascade");
            assert!(
                upper.volumetrics >= lower.volumetrics,
                "{tier} dropped volumetrics"
            );
            assert!(
                upper.soft_shadows || !lower.soft_shadows,
                "{tier} dropped soft shadows"
            );
            assert!(
                upper.contact_shadows || !lower.contact_shadows,
                "{tier} dropped contact shadows"
            );
            assert!(
                upper.contact_shadow_length >= lower.contact_shadow_length,
                "{tier} shortened the contact shadow ray"
            );
            assert!(upper.ssao >= lower.ssao, "{tier} dropped ambient occlusion");
            assert!(upper.ssr || !lower.ssr, "{tier} dropped reflections");
            assert!(upper.lens || !lower.lens, "{tier} dropped the lens stack");
        }
    }

    /// LOD scale is the one number Photo is allowed to break the ladder with,
    /// and only because it is not a playable tier.
    #[test]
    fn detail_is_held_further_out_at_every_playable_tier() {
        let playable = [
            QualityPreset::Low,
            QualityPreset::Medium,
            QualityPreset::High,
            QualityPreset::Ultra,
        ];
        for pair in playable.windows(2) {
            let (lower, upper) = (pair[0].settings(), pair[1].settings());
            assert!(
                upper.lod_scale > lower.lod_scale,
                "{} did not hold detail further out",
                pair[1].name()
            );
            assert!(upper.lod_scale.is_finite());
        }
        assert!(QualityPreset::Photo.settings().lod_scale.is_infinite());
    }

    /// Against the resolved settings a real camera gets, not against the
    /// `raytracing` flag. The flag version of this test passed for as long as
    /// Ultra and Photo shipped with no occlusion term at all, because it took
    /// `raytracing: true` as a promise that something would compute one — and
    /// nothing did.
    #[test]
    fn a_scene_always_has_an_occlusion_term_of_some_kind() {
        for preset in QualityPreset::ALL {
            for caps in [Capabilities::default(), Capabilities::all()] {
                let settings = preset.settings().downgrade(caps);
                assert!(
                    settings.ssao.is_some(),
                    "{} has no ambient occlusion with caps {caps:?}",
                    preset.name()
                );
            }
        }
    }

    /// Every accumulator a preset can name has to be one something attaches.
    /// `Upscaling::Dlss` is not: `render::sync_camera_stack` takes TAA off for
    /// it and puts nothing on, while the shadow filter and the volumetric
    /// jitter both switch to their temporal variants — a frame of crawling
    /// noise with no resolve. Same shape as
    /// `motion_vectors_are_requested_by_every_pass_that_reads_them`.
    #[test]
    fn no_preset_names_a_temporal_pass_that_nothing_attaches() {
        for preset in QualityPreset::ALL {
            assert_ne!(
                preset.settings().upscaling,
                Upscaling::Dlss,
                "{} asks for DLSS, which nothing attaches",
                preset.name()
            );
        }
    }

    /// And if one arrives from a save file anyway, it is walked back rather
    /// than believed.
    #[test]
    fn dlss_is_walked_back_wherever_it_comes_from() {
        let mut settings = QualityPreset::Ultra.settings();
        settings.upscaling = Upscaling::Dlss;
        for caps in [Capabilities::default(), Capabilities::all()] {
            assert_eq!(settings.clone().downgrade(caps).upscaling, Upscaling::Taa);
        }
    }

    /// A temporal resolve softens; a tier that runs one and never sharpens
    /// afterwards is shipping a blur. The converse matters too — sharpening a
    /// frame nothing resolved only sharpens its aliasing.
    #[test]
    fn every_tier_that_resolves_temporally_sharpens_afterwards() {
        for preset in QualityPreset::ALL {
            let settings = preset.settings();
            let temporal = settings.upscaling != Upscaling::Off;
            assert_eq!(
                settings.sharpening > 0.0,
                temporal,
                "{} runs upscaling {:?} with sharpening {}",
                preset.name(),
                settings.upscaling,
                settings.sharpening,
            );
            assert!((0.0..=1.0).contains(&settings.sharpening));
        }
    }

    /// The contact-shadow ray has to be long enough to reach from a wheel to
    /// the road. Bevy's default is 30 cm, which is a third of a wheel.
    #[test]
    fn a_contact_shadow_ray_reaches_the_ground_from_the_things_it_plants() {
        for preset in QualityPreset::ALL {
            let settings = preset.settings();
            if !settings.contact_shadows {
                continue;
            }
            assert!(
                settings.contact_shadow_length >= 0.6,
                "{} plants nothing taller than {} m",
                preset.name(),
                settings.contact_shadow_length
            );
        }
    }

    #[test]
    fn losing_raytracing_falls_back_to_screen_space_rather_than_to_nothing() {
        let bare = Capabilities::default();
        for preset in [QualityPreset::Ultra, QualityPreset::Photo] {
            let settings = preset.settings().downgrade(bare);
            assert!(!settings.raytracing);
            assert!(settings.ssr, "{} lost reflections entirely", preset.name());
            assert!(
                settings.ssao.is_some(),
                "{} lost its occlusion term entirely",
                preset.name()
            );
        }
    }

    #[test]
    fn losing_dlss_leaves_a_temporal_pass_behind() {
        let mut asked = QualityPreset::Ultra.settings();
        asked.upscaling = Upscaling::Dlss;
        let settings = asked.downgrade(Capabilities {
            raytracing: true,
            dlss: false,
        });
        assert_eq!(settings.upscaling, Upscaling::Taa);
        assert!(settings.raytracing, "dlss and raytracing are independent");
    }

    #[test]
    fn downgrading_on_capable_hardware_changes_nothing() {
        for preset in QualityPreset::ALL {
            let settings = preset.settings();
            assert_eq!(settings.clone().downgrade(Capabilities::all()), settings);
        }
    }

    #[test]
    fn downgrading_twice_is_a_no_op() {
        let bare = Capabilities::default();
        for preset in QualityPreset::ALL {
            let once = preset.settings().downgrade(bare);
            assert_eq!(once.clone().downgrade(bare), once);
        }
    }

    #[test]
    fn motion_vectors_are_requested_by_every_pass_that_reads_them() {
        // Low has no temporal pass and no shutter effect, so it is the one tier
        // that can skip the prepass.
        assert!(!QualityPreset::Low.settings().needs_motion_vectors());
        for preset in [
            QualityPreset::Medium,
            QualityPreset::High,
            QualityPreset::Ultra,
            QualityPreset::Photo,
        ] {
            assert!(
                preset.settings().needs_motion_vectors(),
                "{} runs a temporal pass with no motion vectors",
                preset.name()
            );
        }
    }

    #[test]
    fn a_still_is_never_smeared_by_a_shutter() {
        let photo = QualityPreset::Photo.settings();
        assert!(!photo.motion_blur);
    }

    #[test]
    fn lod_distances_scale_with_the_tier() {
        assert_eq!(QualityPreset::High.settings().lod_distance(80.0), 80.0);
        assert!(QualityPreset::Low.settings().lod_distance(80.0) < 80.0);
        assert!(QualityPreset::Ultra.settings().lod_distance(80.0) > 80.0);
        assert!(
            QualityPreset::Photo
                .settings()
                .lod_distance(80.0)
                .is_infinite()
        );
    }
}
