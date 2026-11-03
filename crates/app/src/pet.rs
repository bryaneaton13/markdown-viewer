//! Nib, the bookworm in the document panel. It watches the pointer, blinks
//! when it feels like it, smiles when you come close, hops when clicked and
//! dozes off when left alone. It only exists while the panel is open.

use gpui_kit::{component::*, prelude::FluentBuilder, *};
use std::{
    f32::consts::{PI, TAU},
    time::{Duration, Instant},
};

const SIZE: f32 = 34.;
/// Without pointer movement for this long, Nib naps.
const NAP_AFTER: Duration = Duration::from_secs(14);
/// How long a quip stays before the mood line comes back.
const QUIP_FOR: Duration = Duration::from_millis(3500);

const QUIPS: [&str; 8] = [
    "Boop!",
    "Hey, that tickles.",
    "Wheee!",
    "Headings are my favourite snack.",
    "I've read worse. I've also read better.",
    "Psst: ⌘P finds any file.",
    "Okay, okay, I'm paying attention.",
    "Ask me about footnotes. Actually don't.",
];

pub struct Nib {
    mood: SharedString,
    /// Where the eyes point, each axis in -1..=1.
    look: (f32, f32),
    /// The pointer is right next to Nib.
    near: bool,
    blink: bool,
    asleep: bool,
    last_move: Instant,
    /// Clicks so far; each one restarts the hop and picks the next quip.
    hops: usize,
    quip_at: Option<Instant>,
    seed: u64,
    _life: Task<()>,
}

impl Nib {
    pub fn new(mood: String, cx: &mut Context<Self>) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9, |d| d.as_nanos() as u64)
            | 1;
        let life = cx.spawn(async move |this, cx| {
            loop {
                let Ok(wait) = this.update(cx, |n, _| 2200 + n.roll(3800)) else {
                    break;
                };
                cx.background_executor()
                    .timer(Duration::from_millis(wait))
                    .await;
                // Settle naps and quips, then blink (twice, now and then).
                let Ok(blinks) = this.update(cx, |n, cx| {
                    n.asleep = n.last_move.elapsed() >= NAP_AFTER;
                    if n.quip_at.is_some_and(|t| t.elapsed() >= QUIP_FOR) {
                        n.quip_at = None;
                    }
                    cx.notify();
                    match (n.asleep, n.roll(4)) {
                        (true, _) => 0,
                        (false, 0) => 2,
                        _ => 1,
                    }
                }) else {
                    break;
                };
                for _ in 0..blinks {
                    if this.update(cx, |n, cx| n.set_blink(true, cx)).is_err() {
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    if this.update(cx, |n, cx| n.set_blink(false, cx)).is_err() {
                        return;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(140))
                        .await;
                }
            }
        });
        Self {
            mood: mood.into(),
            look: (0., 0.),
            near: false,
            blink: false,
            asleep: false,
            last_move: Instant::now(),
            hops: 0,
            quip_at: None,
            seed,
            _life: life,
        }
    }

    /// Screenshots: hold a pose (`left`, `up`, `near`, `asleep`, `quip`).
    #[cfg(feature = "screenshot")]
    pub fn pose(&mut self, pose: &str) {
        match pose {
            "left" => self.look = (-1., 0.),
            "up" => self.look = (0.5, -0.9),
            "near" => self.near = true,
            "asleep" => self.asleep = true,
            "quip" => {
                self.hops = 1;
                self.quip_at = Some(Instant::now());
            }
            _ => {}
        }
    }

    /// A small xorshift: Nib needs whims, not cryptography.
    fn roll(&mut self, below: u64) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed % below.max(1)
    }

    fn set_blink(&mut self, blink: bool, cx: &mut Context<Self>) {
        self.blink = blink;
        cx.notify();
    }

    /// The pointer moved somewhere in the window.
    fn watch(&mut self, pointer: Point<Pixels>, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.last_move = Instant::now();
        let center = bounds.center();
        let (dx, dy) = (
            f32::from(pointer.x - center.x),
            f32::from(pointer.y - center.y),
        );
        let dist = (dx * dx + dy * dy).sqrt();
        // Full glance at ~90px away; rounded so tiny moves don't repaint.
        let reach = (dist / 90.).min(1.) / dist.max(1.);
        let quantize = |v: f32| (v * 8.).round() / 8.;
        let look = (quantize(dx * reach), quantize(dy * reach));
        let near = dist < SIZE * 0.9;
        if look != self.look || near != self.near || self.asleep {
            if self.asleep {
                // Startled awake: a blink to shake it off.
                self.blink = false;
            }
            self.look = look;
            self.near = near;
            self.asleep = false;
            cx.notify();
        }
    }

    fn line(&self) -> SharedString {
        if self.asleep {
            "Zzz… (wiggle the pointer)".into()
        } else if self.quip_at.is_some() {
            QUIPS[(self.hops - 1) % QUIPS.len()].into()
        } else {
            self.mood.clone()
        }
    }

    fn eyes(&self) -> Div {
        let ink = rgb(0x1d2b22);
        let e = SIZE * 0.16;
        let eye = || {
            div().flex_shrink_0().map(|d| {
                if self.asleep || self.blink {
                    // Closed: a soft line.
                    d.w(px(e * 1.3)).h(px(1.5)).rounded_full().bg(ink)
                } else if self.near || self.quip_at.is_some() {
                    // Happy: upturned arcs.
                    d.w(px(e * 1.4))
                        .h(px(e * 0.8))
                        .mt(px(e * 0.4))
                        .border_t_2()
                        .border_color(ink)
                        .rounded_t_full()
                } else {
                    d.w(px(e)).h(px(e * 1.3)).rounded_full().bg(ink)
                }
            })
        };
        // Asleep, the eyes drift back to center.
        let (lx, ly) = if self.asleep { (0., 0.) } else { self.look };
        div()
            .relative()
            .left(px(lx * SIZE * 0.1))
            .top(px(ly * SIZE * 0.07))
            .flex()
            .items_center()
            .gap(px(SIZE * 0.16))
            .child(eye())
            .child(eye())
    }

    fn critter(&self) -> AnyElement {
        let happy = (self.near || self.quip_at.is_some()) && !self.asleep;
        let blush = |left: f32| {
            div()
                .absolute()
                .top(px(SIZE * 0.44))
                .left(px(left))
                .w(px(SIZE * 0.14))
                .h(px(SIZE * 0.08))
                .rounded_full()
                .bg(rgba(0xf2788a99))
        };
        let body = div()
            .relative()
            .w(px(SIZE))
            .h(px(SIZE * 0.82))
            .rounded_t(px(SIZE * 0.5))
            .rounded_b(px(SIZE * 0.36))
            .bg(rgb(0x6cc58c))
            .flex()
            .items_center()
            .justify_center()
            .child(self.eyes())
            .when(happy, |d| {
                d.child(blush(SIZE * 0.08)).child(blush(SIZE * 0.78))
            })
            // The sprout, because it eats pages.
            .child(
                div()
                    .absolute()
                    .top(px(-SIZE * 0.14))
                    .left(px(SIZE * 0.58))
                    .w(px(SIZE * 0.18))
                    .h(px(SIZE * 0.2))
                    .rounded_tl(px(SIZE * 0.2))
                    .rounded_br(px(SIZE * 0.2))
                    .bg(rgb(0x3f8f5a)),
            );
        // A gentle bob; slower and shallower while napping.
        let (period, depth) = if self.asleep {
            (4200, 0.03)
        } else {
            (2400, 0.08)
        };
        let bobbing = div()
            .absolute()
            .top(px(SIZE * 0.1))
            .child(body)
            .with_animation(
                ElementId::Name(format!("nib-bob-{}", self.asleep).into()),
                Animation::new(Duration::from_millis(period))
                    .repeat()
                    .with_max_fps(30.),
                move |d, t| d.top(px(SIZE * (0.1 - depth * (t * TAU).sin().abs()))),
            );
        if self.hops == 0 {
            return bobbing.into_any_element();
        }
        // A click: one springy hop, restarted by every click.
        div()
            .absolute()
            .inset_0()
            .child(bobbing)
            .with_animation(
                ("nib-hop", self.hops),
                Animation::new(Duration::from_millis(480)),
                |d, t| d.top(px(-SIZE * 0.32 * (t * PI).sin())),
            )
            .into_any_element()
    }
}

impl Render for Nib {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (shade, muted) = (theme.foreground, theme.muted_foreground);
        let weak = cx.entity().downgrade();
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .id("nib")
                    .flex_shrink_0()
                    .relative()
                    .w(px(SIZE))
                    .h(px(SIZE * 1.1))
                    .cursor_pointer()
                    // Listen to the pointer anywhere in the window, from
                    // where Nib sits.
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Bubble {
                                        let _ = weak
                                            .update(cx, |n, cx| n.watch(e.position, bounds, cx));
                                    }
                                });
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    // Shadow: narrower while hopping would be nice, but a
                    // steady one reads as "on the ground" well enough.
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(SIZE * 0.15))
                            .w(px(SIZE * 0.7))
                            .h(px(SIZE * 0.1))
                            .rounded_full()
                            .bg(shade.opacity(0.12)),
                    )
                    .child(self.critter())
                    .when(self.asleep, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(-SIZE * 0.2))
                                .left(px(SIZE * 0.9))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(muted)
                                .child("z")
                                .with_animation(
                                    "nib-z",
                                    Animation::new(Duration::from_millis(2600))
                                        .repeat()
                                        .with_max_fps(20.),
                                    |d, t| d.top(px(-SIZE * (0.05 + 0.35 * t))).opacity(1. - t),
                                ),
                        )
                    })
                    .on_click(cx.listener(|n, _, _, cx| {
                        n.hops += 1;
                        n.quip_at = Some(Instant::now());
                        n.asleep = false;
                        n.last_move = Instant::now();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Nib"))
                    .child(div().text_xs().text_color(muted).child(self.line())),
            )
    }
}
