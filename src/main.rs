pub mod adjusted_color;
pub mod draw_buffer;
use anyhow::{anyhow, Context, Error, Result};
pub mod image;
use tokio::time::{sleep, Duration};
pub mod next_buses;
pub mod pusher;

use crate::draw_buffer::draw_buffer::get_rgba;
use crate::next_buses::get_next_buses;
use adjusted_color::adjusted_color::adjusted_color;
use chrono::prelude::*;
use clap::Parser;
use dotenv::dotenv;
use pusher::pusher::push;
use raqote::*;
use webp::{AnimEncoder, AnimFrame, WebPConfig};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Filename of the debug file
    #[arg(short, long)]
    debug: Option<String>,
}

// Built in 2px of buffer.
const WIDTH: i64 = 61;

#[derive(Copy, Clone)]
enum TextAlign {
    Left,
    Right,
}

fn draw_text(
    dt: &mut DrawTarget,
    text: &str,
    in_start: Point,
    color: &Source,
    align: TextAlign,
) -> Result<()> {
    let tom_thumb = include_bytes!("../fonts/tb-8.bdf");
    let font: bdf::Font = bdf::read(&tom_thumb[..])?;
    let mut start = in_start.clone();
    let chars: Vec<char> = match align {
        TextAlign::Left => text.chars().collect(),
        TextAlign::Right => text.chars().rev().collect(),
    };
    for c in chars {
        let glyph = font.glyphs().get(&c).context("Could not get glyph")?;

        // The tom-thumb font is monospace but some of the characters
        // don't take up the full bounding box. Offset them so that
        // they sit at the center of their bits.

        for px in glyph.pixels() {
            let x = px.0 .0;
            let y = px.0 .1;
            let white = px.1;
            if white {
                dt.fill_rect(
                    start.x + x as f32 + 0 as f32,
                    start.y + y as f32 + 0 as f32,
                    1.,
                    1.,
                    color,
                    &DrawOptions::new(),
                )
            }
        }
        start.x = start.x
            + advance(c)
                * match align {
                    TextAlign::Left => 1.0,
                    TextAlign::Right => -1.0,
                }
    }
    Ok(())
}

fn advance(c: char) -> f32 {
    if c == ' ' {
        return 2.0;
    }
    let tom_thumb = include_bytes!("../fonts/tb-8.bdf");
    let font = bdf::read(&tom_thumb[..]);
    let w = font.map(|f| f.glyphs().get(&c).map(|g| g.width()));
    w.unwrap_or(Some(0)).unwrap_or(0) as f32
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let args = Args::parse();
    let ten_seconds = Duration::from_secs(10);

    loop {
        render(&args).await;

        if args.debug.is_some() {
            break;
        }

        sleep(ten_seconds).await;
    }
}

trait Widget: Send {
    // Gets the width of the given widget
    fn measure(&self) -> Point;
    fn frame_count(&self) -> u32;
    fn render(&self, dt: &mut DrawTarget, point: Point, frame: u32) -> Result<(), Error>;
}

#[derive(Copy, Clone, Debug)]
pub struct TextWidget<'a> {
    text: &'a str,
    color: &'a str,
}

impl<'a> TextWidget<'a> {
    fn new(text: &'a str, color: &'a str) -> Result<TextWidget<'a>, Error> {
        Ok::<TextWidget<'a>, Error>(TextWidget { text, color })
    }
}

impl<'a> Widget for TextWidget<'a> {
    fn measure(&self) -> Point {
        let width: f32 = self.text.chars().map(|x| advance(x)).sum();
        Point::new(width, 8.0)
    }
    fn frame_count(&self) -> u32 {
        1
    }
    fn render(&self, dt: &mut DrawTarget, point: Point, frame: u32) -> Result<(), Error> {
        let color = adjusted_color(&self.color)?;
        draw_text(dt, &self.text, point, &color, TextAlign::Left)
    }
}

struct ChartWidget {
    data: Vec<u64>,
    height: i32,
}

impl ChartWidget {
    fn new(data: &Vec<u64>) -> Result<ChartWidget, anyhow::Error> {
        Ok::<ChartWidget, anyhow::Error>(ChartWidget {
            data: data.clone(),
            height: 5,
        })
    }
}

impl Widget for ChartWidget {
    fn measure(&self) -> Point {
        Point::new(self.data.len() as f32, 8.0)
    }
    fn frame_count(&self) -> u32 {
        1
    }
    fn render(&self, dt: &mut DrawTarget, point: Point, frame: u32) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        let mut pt = point.clone();
        for d in &self.data {
            let mut h = (d + 1) as f32;
            let high = h > 8.0;
            if high {
                h = 8.0;
            }
            let color = adjusted_color(if high {
                "#0ff"
            } else if h > 1.0 {
                "#eee"
            } else {
                "#555"
            })?;
            dt.fill_rect(
                pt.x,
                pt.y + (self.height as f32) - h,
                1.0,
                h,
                &color,
                &DrawOptions::new(),
            );
            pt.x += 1.0;
        }
        Ok(())
    }
}

/**
 * Horizontal stack
 */
struct HStack {
    items: Vec<Box<dyn Widget>>,
    gap: f32,
    expand: bool,
}

impl HStack {
    fn set_gap(mut self, gap: f32) -> HStack {
        self.gap = gap;
        self
    }
    fn set_expand(mut self, expand: bool) -> HStack {
        self.expand = expand;
        self
    }
}

impl Widget for HStack {
    fn measure(&self) -> Point {
        let max_height = self
            .items
            .iter()
            .map(|item| item.measure().y.round() as u32)
            .max()
            .unwrap_or(5) as f32;
        Point::new(WIDTH as f32, max_height)
    }
    fn frame_count(&self) -> u32 {
        self.items
            .iter()
            .map(|item| item.frame_count())
            .max()
            .unwrap_or(1)
    }
    fn render(&self, dt: &mut DrawTarget, point: Point, frame: u32) -> Result<()> {
        if self.items.is_empty() {
        } else if self.items.len() == 1 {
            if let Some(item) = self.items.first() {
                item.render(dt, point, frame)?;
            }
        } else {
            let widths: Vec<u32> = self
                .items
                .iter()
                .map(|item| item.measure().x.round() as u32)
                .collect();
            let total_content_size = widths.iter().sum::<u32>() as i64;
            let extra_room: i64 = WIDTH - total_content_size;
            let gap_count = (self.items.len() - 1) as i64;
            let mut spaces: Vec<f32> = Vec::new();
            let space_between = extra_room / gap_count;
            spaces.resize(self.items.len() - 1, space_between as f32);
            let total_size_with_gaps = total_content_size + (gap_count * space_between);
            let remainder = WIDTH - total_size_with_gaps;
            // Add any extra remainder space to the last gap.
            if remainder > 0 {
                if let Some(last) = spaces.last_mut() {
                    *last = *last + remainder as f32;
                }
            }
            let mut start_point = point.clone();
            for (i, item) in self.items.iter().enumerate() {
                item.render(dt, start_point, frame)?;
                start_point.x = start_point.x + item.measure().x + spaces.get(i).unwrap_or(&0.0);
            }
        }
        Ok(())
    }
}

/**
 * Horizontal stack
 */
struct VStack {
    items: Vec<Box<dyn Widget>>,
    gap: f32,
}

impl VStack {
    fn set_gap(mut self, gap: f32) -> VStack {
        self.gap = gap;
        self
    }
}

impl Widget for VStack {
    fn measure(&self) -> Point {
        Point::new(WIDTH as f32, 5.0)
    }
    fn frame_count(&self) -> u32 {
        self.items
            .iter()
            .map(|item| item.frame_count())
            .max()
            .unwrap_or(1)
    }
    fn render(&self, dt: &mut DrawTarget, point: Point, frame: u32) -> Result<()> {
        let mut start_point = point.clone();
        for item in self.items.iter() {
            item.render(dt, start_point, frame)?;
            start_point.y = start_point.y + item.measure().y + self.gap;
        }
        Ok(())
    }
}

// It's super annoying to create a Vec of things
// that implement the Widget trait, so trying to use
// a macro instead.
#[macro_export]
macro_rules! hstack {
    ( $( $x:expr ),* ) => {
        {
            let mut temp_vec: Vec<Box<dyn Widget>> = Vec::new();
            $(
                if let Ok(z) = $x {
                    temp_vec.push(Box::new(z));
                }
                if let Err(z) = $x {
                    println!("{:?}", z);
                }
            )*
            let res: Result<HStack, anyhow::Error> = Ok(HStack {
                items: temp_vec,
                gap: 0.0,
                expand: false
            });
            res
        }
    };
}

#[macro_export]
macro_rules! vstack {
    ( $( $x:expr ),* ) => {
        {
            let mut temp_vec: Vec<Box<dyn Widget>> = Vec::new();
            $(
                if let Ok(z) = $x {
                    temp_vec.push(Box::new(z));
                }
            )*
            let res: Result<VStack, anyhow::Error> = Ok(VStack { items: temp_vec, gap: 0.0 });
            res
        }
    };
}

macro_rules! vstack {
    ( $( $x:expr ),* ) => {
        {
            let mut temp_vec: Vec<Box<dyn Widget>> = Vec::new();
            $(
                if let Ok(z) = $x {
                    temp_vec.push(Box::new(z));
                }
            )*
            let res: Result<VStack, anyhow::Error> = Ok(VStack { items: temp_vec, gap: 0.0 });
            res
        }
    };
}

async fn render(args: &Args) -> Result<()> {
    let local: DateTime<Local> = Local::now();
    let now: DateTime<FixedOffset> = local.into();
    let width = 64i32;
    let height = 32i32;
    let mut config = WebPConfig::new().map_err(|_s| anyhow!("WebPConfig failed"))?;
    config.lossless = 1;
    let mut encoder = AnimEncoder::new(width as u32, height as u32, &config);

    let next_buses = get_next_buses().await?;

    let [widget1, widget2, widget3, widget4, widget5, widget6]: [TextWidget; 6] = next_buses
        .iter()
        .flat_map(|arrival| {
            vec![
                TextWidget::new(format!("{}", arrival.line).as_str(), "#fff").unwrap(),
                TextWidget::new(
                    format!("{} min", (arrival.expected_time - now).num_minutes()).as_str(),
                    "#fff",
                )
                .unwrap(),
            ]
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();

    let layout = vstack![
        hstack![
            Ok::<TextWidget, Error>(widget1),
            Ok::<TextWidget, Error>(widget2)
        ],
        hstack![
            Ok::<TextWidget, Error>(widget3),
            Ok::<TextWidget, Error>(widget4)
        ],
        hstack![
            Ok::<TextWidget, Error>(widget5),
            Ok::<TextWidget, Error>(widget6)
        ]
    ]
    .map(|s| s.set_gap(2.0));

    let mut frames: Vec<Vec<u8>> = Vec::new();

    if let Ok(l) = layout {
        let frame_count = l.frame_count();
        println!("Frame count: {:?}", frame_count);
        for frame in 0..frame_count {
            let mut dt = DrawTarget::new(width, height);
            l.render(&mut dt, Point::new(2., 2.), frame)?;

            let output = get_rgba(dt);
            frames.push(output);
        }
    }

    // Step 1: Clone all outputs
    let cloned_outputs: Vec<_> = frames.into_iter().map(|output| output.clone()).collect();

    // Step 2: Create AnimFrames
    let frames: Vec<_> = cloned_outputs
        .iter()
        .map(|rgba| AnimFrame::from_rgba(rgba, width as u32, height as u32, 0))
        .collect();

    // Step 3: Add each frame to the encoder
    let _f: Vec<_> = frames
        .into_iter()
        .map(|frame| encoder.add_frame(frame))
        .collect();

    let file_contents = encoder.encode().to_vec();

    if let Some(filename) = &args.debug {
        std::fs::write(filename, file_contents)?;
    } else {
        push(&file_contents).await?;
    }
    Ok(())
}
