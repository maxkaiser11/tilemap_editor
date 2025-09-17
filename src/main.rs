use eframe::egui::ColorImage;
use eframe::{egui, egui::Vec2};
use image::GenericImage;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(Vec2::new(1200.0, 800.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Goob TileMap Editor",
        native_options,
        Box::new(|_cc| {
            // Return Result<Box<dyn eframe::App>, Box<dyn Error + Send + Sync>>
            Ok(Box::new(App::default()) as Box<dyn eframe::App>)
        }),
    )
}

struct App {
    zoom: f32,
    camera: glam::Vec2,
    show_grid: bool,
    tile_size: u32,
    margin: u32,
    spacing: u32,
    tileset_path: Option<String>,
    tileset_tex: Option<egui::TextureHandle>,
    tileset_dims: (u32, u32),
    tileset_cols_rows: (u32, u32),
    selected_tile: i32, // -1 = eraser
    // Map (one layer)
    map_size: (u32, u32), // tiles (w,h)
    map_data: Vec<i32>,   // row-major
    tileset_rgba: Option<image::RgbaImage>,
}

impl Default for App {
    fn default() -> Self {
        let w = 64;
        let h = 48;
        Self {
            zoom: 1.0,
            camera: glam::vec2(0.0, 0.0),
            show_grid: true,

            tile_size: 32,
            margin: 0,
            spacing: 0,
            tileset_path: None,
            tileset_tex: None,
            tileset_dims: (0, 0),
            tileset_cols_rows: (0, 0),

            selected_tile: -1,
            map_size: (w, h),
            map_data: vec![-1; (w * h) as usize],
            tileset_rgba: None,
        }
    }
}

impl App {
    fn ui_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open Tileset…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("png", &["png"])
                        .pick_file()
                    {
                        self.load_tileset(ctx, path.to_string_lossy().to_string());
                    }
                }
                if ui.button("Export PNG…").clicked() {
                    // default name
                    let suggested = std::path::PathBuf::from("tilemap.png");
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(suggested.file_name().unwrap().to_string_lossy())
                        .add_filter("png", &["png"])
                        .save_file()
                    {
                        if let Err(e) = self.export_map_png(&path) {
                            eprintln!("Export failed: {e}");
                        } else {
                            println!("Exported: {}", path.display());
                        }
                    }
                }
                ui.separator();
                ui.label("Tile size:");
                ui.add(egui::DragValue::new(&mut self.tile_size).clamp_range(4..=256));
                ui.checkbox(&mut self.show_grid, "Grid");
                ui.separator();
                if ui.button("New Map 64x48").clicked() {
                    self.map_size = (64, 48);
                    self.map_data = vec![-1; (64 * 48) as usize];
                    self.camera = glam::vec2(0.0, 0.0);
                    self.zoom = 1.0;
                }
            });
        });
    }

    fn load_tileset(&mut self, ctx: &egui::Context, path: String) {
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8(); // keep this so we can export later
                let (w, h) = rgba.dimensions();

                // egui texture (copied from bytes)
                let color_image =
                    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
                let tex = ctx.load_texture("tileset", color_image, egui::TextureOptions::NEAREST);

                self.tileset_tex = Some(tex);
                self.tileset_path = Some(path);
                self.tileset_dims = (w, h);
                self.tileset_rgba = Some(rgba); // <- keep pixels
                self.recompute_cols_rows();
            }
            Err(e) => eprintln!("Failed to load tileset: {e}"),
        }
    }

    fn recompute_cols_rows(&mut self) {
        let (w, h) = self.tileset_dims;
        if self.tile_size == 0 {
            return;
        }
        let t = self.tile_size;
        let s = self.spacing;
        let m = self.margin;

        // Tiled-style formula
        let cols = if w >= 2 * m + t {
            (w - 2 * m + s) / (t + s)
        } else {
            0
        };
        let rows = if h >= 2 * m + t {
            (h - 2 * m + s) / (t + s)
        } else {
            0
        };

        self.tileset_cols_rows = (cols, rows);
    }

    fn ui_palette(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("palette")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Palette");

                if let Some(tex) = &self.tileset_tex {
                    // Cast usize -> f32 for math
                    let tex_w_f = tex.size()[0] as f32;
                    let tex_h_f = tex.size()[1] as f32;

                    let avail_w = ui.available_width();
                    let scale = (avail_w / tex_w_f).max(0.1);
                    let img_size = egui::vec2(tex_w_f * scale, tex_h_f * scale);

                    // Show the full tileset image
                    let resp = ui
                        .add(egui::Image::new(tex).fit_to_exact_size(img_size))
                        .interact(egui::Sense::click_and_drag());

                    // Draw grid overlay
                    let painter = ui.painter_at(resp.rect);
                    let (cols, rows) = self.tileset_cols_rows;
                    if cols > 0 && rows > 0 && self.tile_size > 0 {
                        let step_x = (self.tile_size + self.spacing) as f32 * scale;
                        let step_y = (self.tile_size + self.spacing) as f32 * scale;
                        let start = resp.rect.min
                            + egui::vec2(self.margin as f32 * scale, self.margin as f32 * scale);
                        let color = egui::Color32::from_gray(120);

                        for c in 0..=cols {
                            let x = start.x + c as f32 * step_x;
                            painter.line_segment(
                                [
                                    egui::pos2(x, start.y),
                                    egui::pos2(x, start.y + rows as f32 * step_y),
                                ],
                                (1.0, color),
                            );
                        }
                        for r in 0..=rows {
                            let y = start.y + r as f32 * step_y;
                            painter.line_segment(
                                [
                                    egui::pos2(start.x, y),
                                    egui::pos2(start.x + cols as f32 * step_x, y),
                                ],
                                (1.0, color),
                            );
                        }
                    }

                    // Click → select tile
                    if resp.clicked() || resp.dragged() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            if let Some(tile) = self.pick_tile_from_palette(pos, resp.rect, scale) {
                                self.selected_tile = tile as i32;
                            }
                        }
                    }

                    ui.label(format!("Selected tile: {}", self.selected_tile));
                    if ui.button("Eraser").clicked() {
                        self.selected_tile = -1;
                    }
                } else {
                    ui.label("Open a tileset PNG (top bar).");
                }
            });
    }

    fn pick_tile_from_palette(
        &self,
        mouse: egui::Pos2,
        rect: egui::Rect,
        scale: f32,
    ) -> Option<u32> {
        let (cols, rows) = self.tileset_cols_rows;
        if cols == 0 || rows == 0 {
            return None;
        }
        let start = rect.min + egui::vec2(self.margin as f32 * scale, self.margin as f32 * scale);
        let step_x = (self.tile_size + self.spacing) as f32 * scale;
        let step_y = (self.tile_size + self.spacing) as f32 * scale;

        let rel = mouse - start;
        if rel.x < 0.0 || rel.y < 0.0 {
            return None;
        }
        let c = (rel.x / step_x).floor() as i32;
        let r = (rel.y / step_y).floor() as i32;
        if c < 0 || r < 0 {
            return None;
        }
        let (c, r) = (c as u32, r as u32);
        if c >= cols || r >= rows {
            return None;
        }
        Some(r * cols + c)
    }

    fn ui_viewport(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.style_mut().visuals.clip_rect_margin = 0.0;
            let avail = ui.available_size();
            let (resp, painter) = ui.allocate_painter(avail, egui::Sense::click_and_drag());

            // zoom on wheel (zoom around mouse)
            if resp.hovered() {
                if ui.input(|i| i.zoom_delta() != 1.0) {
                    let mouse = ui
                        .input(|i| i.pointer.hover_pos())
                        .unwrap_or(resp.rect.center());
                    let world_before = self.screen_to_world(mouse, resp.rect);
                    self.zoom = (self.zoom * ui.input(|i| i.zoom_delta())).clamp(0.25, 8.0);
                    let world_after = self.screen_to_world(mouse, resp.rect);
                    let delta = world_after - world_before;
                    self.camera -= glam::vec2(delta.x, delta.y);
                }
            }

            // pan with middle-drag
            if resp.dragged_by(egui::PointerButton::Middle) {
                let delta = resp.drag_delta();
                self.camera -= glam::vec2(delta.x, delta.y) / self.zoom;
            }

            // paint with LMB/RMB
            if resp.clicked_by(egui::PointerButton::Primary)
                || resp.dragged_by(egui::PointerButton::Primary)
            {
                if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                    self.paint_at(mouse, resp.rect, self.selected_tile);
                }
            }
            if resp.clicked_by(egui::PointerButton::Secondary)
                || resp.dragged_by(egui::PointerButton::Secondary)
            {
                if let Some(mouse) = ui.input(|i| i.pointer.hover_pos()) {
                    self.paint_at(mouse, resp.rect, -1);
                }
            }

            // draw background
            painter.rect_filled(resp.rect, 0.0, egui::Color32::from_gray(32));

            // draw tilemap using tileset texture
            if let Some(tex) = &self.tileset_tex {
                self.draw_tilemap(&painter, resp.rect, tex);
            }

            // grid overlay
            if self.show_grid {
                self.draw_grid(&painter, resp.rect);
            }
        });
    }

    fn screen_to_world(&self, screen: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let p = (screen - rect.left_top()) / self.zoom;
        egui::pos2(p.x + self.camera.x, p.y + self.camera.y)
    }

    fn world_to_screen(&self, world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let p = egui::pos2(world.x - self.camera.x, world.y - self.camera.y) * self.zoom
            + rect.left_top().to_vec2();
        egui::pos2(p.x, p.y)
    }

    fn paint_at(&mut self, mouse_screen: egui::Pos2, rect: egui::Rect, tile: i32) {
        let world = self.screen_to_world(mouse_screen, rect);
        let ts = self.tile_size as f32;
        let tx = ((world.x / ts).floor() as i32).max(0);
        let ty = ((world.y / ts).floor() as i32).max(0);
        let (mw, mh) = (self.map_size.0 as i32, self.map_size.1 as i32);
        if tx >= 0 && ty >= 0 && tx < mw && ty < mh {
            let idx = (ty as u32 * self.map_size.0 + tx as u32) as usize;
            self.map_data[idx] = tile;
        }
    }

    fn draw_tilemap(&self, painter: &egui::Painter, rect: egui::Rect, tex: &egui::TextureHandle) {
        let (mw, mh) = self.map_size;
        if mw == 0 || mh == 0 || self.tile_size == 0 {
            return;
        }

        let ts = self.tile_size as f32;
        let (cols, _rows) = self.tileset_cols_rows;
        if cols == 0 {
            return;
        }

        // Cast texture size once to f32
        let tex_w_f = tex.size()[0] as f32;
        let tex_h_f = tex.size()[1] as f32;

        let mut mesh = egui::Mesh::with_texture(tex.id());

        for y in 0..mh {
            for x in 0..mw {
                let id = self.map_data[(y * mw + x) as usize];
                if id < 0 {
                    continue;
                }
                let id = id as u32;

                // tile column/row inside the tileset
                let c = id % cols;
                let r = id / cols;

                // pixel position of that tile in the tileset (u32 math)
                let px = self.margin + c * (self.tile_size + self.spacing);
                let py = self.margin + r * (self.tile_size + self.spacing);

                // UVs (cast to f32 for division)
                let u0 = px as f32 / tex_w_f;
                let v0 = py as f32 / tex_h_f;
                let u1 = (px + self.tile_size) as f32 / tex_w_f;
                let v1 = (py + self.tile_size) as f32 / tex_h_f;

                // world quad in pixels
                let x0 = x as f32 * ts;
                let y0 = y as f32 * ts;
                let x1 = x0 + ts;
                let y1 = y0 + ts;

                // to screen space
                let p0 = self.world_to_screen(egui::pos2(x0, y0), rect);
                let p1 = self.world_to_screen(egui::pos2(x1, y0), rect);
                let p2 = self.world_to_screen(egui::pos2(x1, y1), rect);
                let p3 = self.world_to_screen(egui::pos2(x0, y1), rect);

                // append quad
                let base = mesh.vertices.len() as u32;
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p0,
                    uv: egui::pos2(u0, v0),
                    color: egui::Color32::WHITE,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p1,
                    uv: egui::pos2(u1, v0),
                    color: egui::Color32::WHITE,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p2,
                    uv: egui::pos2(u1, v1),
                    color: egui::Color32::WHITE,
                });
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p3,
                    uv: egui::pos2(u0, v1),
                    color: egui::Color32::WHITE,
                });
                mesh.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            }
        }

        painter.add(egui::Shape::mesh(mesh));
    }

    fn draw_grid(&self, painter: &egui::Painter, rect: egui::Rect) {
        let ts = self.tile_size as f32;
        let color = egui::Color32::from_gray(70);
        let thickness = 1.0;

        // visible world bounds (approx)
        let top_left_world = self.screen_to_world(rect.left_top(), rect);
        let bottom_right_world = self.screen_to_world(rect.right_bottom(), rect);
        let min_tx = (top_left_world.x / ts).floor() as i32 - 1;
        let min_ty = (top_left_world.y / ts).floor() as i32 - 1;
        let max_tx = (bottom_right_world.x / ts).ceil() as i32 + 1;
        let max_ty = (bottom_right_world.y / ts).ceil() as i32 + 1;

        for x in min_tx..=max_tx {
            let wx = x as f32 * ts;
            let p0 = self.world_to_screen(egui::pos2(wx, top_left_world.y), rect);
            let p1 = self.world_to_screen(egui::pos2(wx, bottom_right_world.y), rect);
            painter.line_segment([p0, p1], (thickness, color));
        }
        for y in min_ty..=max_ty {
            let wy = y as f32 * ts;
            let p0 = self.world_to_screen(egui::pos2(top_left_world.x, wy), rect);
            let p1 = self.world_to_screen(egui::pos2(bottom_right_world.x, wy), rect);
            painter.line_segment([p0, p1], (thickness, color));
        }
    }

    fn export_map_png(&self, out_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        use image::{RgbaImage, imageops};

        let (mw, mh) = self.map_size;
        let t = self.tile_size;
        if mw == 0 || mh == 0 || t == 0 {
            return Err("map size or tile size is zero".into());
        }
        let tileset = self
            .tileset_rgba
            .as_ref()
            .ok_or("no tileset loaded (open a PNG first)")?;
        let (cols, _rows) = self.tileset_cols_rows;
        if cols == 0 {
            return Err("computed 0 columns — check tile size/margin/spacing".into());
        }

        let out_w = mw * t;
        let out_h = mh * t;
        let mut out = RgbaImage::from_pixel(out_w, out_h, image::Rgba([0, 0, 0, 0])); // transparent bg

        // Copy each placed tile into the output
        for y in 0..mh {
            for x in 0..mw {
                let id = self.map_data[(y * mw + x) as usize];
                if id < 0 {
                    continue;
                }
                let id = id as u32;

                // tile col/row inside tileset
                let c = id % cols;
                let r = id / cols;

                // source pixel position in tileset (respect margin/spacing)
                let px = self.margin + c * (self.tile_size + self.spacing);
                let py = self.margin + r * (self.tile_size + self.spacing);

                // Guard against out-of-bounds (defensive)
                if px + t > tileset.width() || py + t > tileset.height() {
                    continue;
                }

                // Grab tile image and paste to output
                let tile_img = imageops::crop_imm(tileset, px, py, t, t).to_image();
                let dx = x * t;
                let dy = y * t;
                let _ = out.copy_from(&tile_img, dx, dy);
            }
        }

        // Write PNG
        out.save(out_path)?;
        Ok(())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // recompute cols/rows if tile_size changed
        // (put this here to respond to DragValue in the top bar)
        self.recompute_cols_rows();

        self.ui_top_bar(ctx);
        self.ui_palette(ctx);
        self.ui_viewport(ctx);
    }
}
