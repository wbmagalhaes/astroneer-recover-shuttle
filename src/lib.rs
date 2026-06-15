//! Astroneer .savegame decoder + FORCE LAND fix, in Rust (compiles to WASM for the browser).

use miniz_oxide::inflate::decompress_to_vec_zlib;

const RAISE: f32 = 40.0; // lift the ship slightly along the surface normal so it isn't embedded

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn u8(&mut self) -> u8 {
        let v = self.b[self.p];
        self.p += 1;
        v
    }

    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes([self.b[self.p], self.b[self.p + 1]]);
        self.p += 2;
        v
    }

    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        v
    }

    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }

    fn i64(&mut self) -> i64 {
        let v = i64::from_le_bytes(self.b[self.p..self.p + 8].try_into().unwrap());
        self.p += 8;
        v
    }

    fn skip(&mut self, n: usize) {
        self.p += n;
    }

    fn string(&mut self) -> String {
        let n = self.i32();
        let mut s = String::new();
        for _ in 0..n {
            s.push(self.u8() as char);
        }
        s
    }
}

fn clean(s: &str) -> &str {
    s.trim_end_matches('\0')
}

pub struct ObjRec {
    pub object_type: String,
    pub name_index: i32,
    pub data_off: usize, // absolute offset of `data` blob in decompressed buffer
    pub data_len: usize,
    pub custom_off: usize,
    pub custom_len: usize,
}

pub struct ActorRec {
    pub object_index: i32,
    pub components: Vec<(i32, i32)>, // (name_index, object_index)
    pub rot_off: usize,              // 4 f32
    pub trans_off: usize,            // 3 f32
}

pub struct Save {
    pub header: Vec<u8>, // 16-byte outer header
    pub raw: Vec<u8>,    // decompressed
    pub strings: Vec<String>,
    pub objects: Vec<ObjRec>,
    pub actors: Vec<ActorRec>,
    pub abo: std::collections::HashMap<i32, usize>,
    pub host_pawn: Option<i32>, // host pawn OBJECT index (from tail player-controller record)
}

impl Save {
    pub fn name(&self, i: i32) -> String {
        if i == 0 {
            "None".into()
        } else {
            self.strings
                .get((i - 1) as usize)
                .map(|s| clean(s).to_string())
                .unwrap_or_default()
        }
    }

    fn find_string_index(&self, target: &str) -> Option<i32> {
        self.strings
            .iter()
            .position(|s| clean(s) == target)
            .map(|p| (p + 1) as i32)
    }

    pub fn actor_of(&self, obj_index: usize) -> Option<&ActorRec> {
        self.abo
            .get(&(obj_index as i32))
            .map(|&ai| &self.actors[ai])
    }

    fn comp_obj(&self, obj_index: usize, comp_name: &str) -> Option<usize> {
        let a = self.actor_of(obj_index)?;
        for &(ni, oi) in &a.components {
            if self.name(ni) == comp_name {
                return Some(oi as usize);
            }
        }
        None
    }

    fn read_f32(&self, off: usize) -> f32 {
        f32::from_le_bytes(self.raw[off..off + 4].try_into().unwrap())
    }

    fn trans(&self, obj_index: usize) -> Option<[f32; 3]> {
        let a = self.actor_of(obj_index)?;
        Some([
            self.read_f32(a.trans_off),
            self.read_f32(a.trans_off + 4),
            self.read_f32(a.trans_off + 8),
        ])
    }
}

pub fn parse(blob: &[u8]) -> Result<Save, String> {
    if blob.len() < 16 {
        return Err("file too small".into());
    }

    let header = blob[..16].to_vec();
    let raw =
        decompress_to_vec_zlib(&blob[16..]).map_err(|e| format!("zlib inflate failed: {:?}", e))?;
    let mut c = Cur { b: &raw, p: 0 };

    // ---- GVAS header ----
    c.u32(); // format_tag
    c.i32(); // save_game_version
    c.i32(); // package_version
    c.skip(6); // engine_version major/minor/patch (3x u16)
    c.u32(); // engine build
    c.string(); // build_id
    c.i32(); // custom_format_data.version
    let cfd_count = c.u32();
    for _ in 0..cfd_count {
        c.skip(16);
        c.i32();
    } // u128 guid + i32
    c.string(); // save_class
    c.string(); // end_of_header1
    c.i32(); // end_of_header2

    // ---- level prefix ----
    c.u32(); // astro_save_version
    c.u8(); // pad1
    c.string(); // level_name
    c.u32(); // pad2

    // ---- level chunk ----
    c.u32(); // chunk astro_save_version
    let str_count = c.i64();
    let mut strings = Vec::with_capacity((str_count.max(1) - 1) as usize);
    for _ in 0..(str_count - 1) {
        strings.push(c.string());
    }

    let obj_count = c.u32();
    let mut objects = Vec::with_capacity(obj_count as usize);
    for _ in 0..obj_count {
        let object_type = c.string();
        let name_index = c.i32();
        c.u32(); // flags
        let save_flags = c.u8();
        c.i32(); // outer_object_index
        let custom_data_offset = c.u32() as usize;
        let size = if save_flags & 4 != 0 {
            c.u32() as usize
        } else {
            0
        };
        let data_off = c.p;
        c.skip(custom_data_offset);
        let (custom_off, custom_len) = if size != 0 {
            let off = c.p;
            let len = size - custom_data_offset;
            c.skip(len);
            (off, len)
        } else {
            (c.p, 0)
        };
        objects.push(ObjRec {
            object_type,
            name_index,
            data_off,
            data_len: custom_data_offset,
            custom_off,
            custom_len,
        });
    }

    let act_count = c.u32();
    let mut actors = Vec::with_capacity(act_count as usize);
    let mut abo = std::collections::HashMap::new();
    for ai in 0..act_count as usize {
        let object_index = c.i32();
        let child_count = c.i32();
        for _ in 0..child_count {
            c.i32();
            c.i32();
        }
        let comp_count = c.i32();
        let mut components = Vec::with_capacity(comp_count.max(0) as usize);
        for _ in 0..comp_count {
            let ni = c.i32();
            let oi = c.i32();
            components.push((ni, oi));
        }
        let rot_off = c.p;
        c.skip(16); // rotation 4 f32
        let trans_off = c.p;
        c.skip(12); // translation 3 f32
        c.skip(12); // scale 3 f32
        abo.insert(object_index, ai);
        actors.push(ActorRec {
            object_index,
            components,
            rot_off,
            trans_off,
        });
    }

    // root_level_actor_indices + first_import_index, then the tail's player-controller record
    let root_count = c.u32();
    c.skip(4 * root_count as usize);
    c.u32(); // first_import_index
    let mut host_pawn: Option<i32> = None;
    if c.p + 4 <= raw.len() {
        let pcr_count = c.i32();
        if pcr_count > 0 && c.p + 16 <= raw.len() {
            c.u32(); // controller actor index
            host_pawn = Some(c.u32() as i32); // last_controller_pawn = OBJECT index
        }
    }

    Ok(Save {
        header,
        raw,
        strings,
        objects,
        actors,
        abo,
        host_pawn,
    })
}

#[derive(Clone)]
pub struct Prop {
    pub path: String,
    pub ptype: String,
    pub voff: usize,
}

pub fn decode_props(blob: &[u8], save: &Save) -> Vec<Prop> {
    let mut out = Vec::new();
    stream(blob, 0, blob.len(), "", save, &mut out);
    out
}

fn rd_i32(b: &[u8], p: usize) -> i32 {
    i32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn rd_i64(b: &[u8], p: usize) -> i64 {
    i64::from_le_bytes(b[p..p + 8].try_into().unwrap())
}

fn native_floats(stype: &str) -> Option<usize> {
    match stype {
        "Quat" => Some(4),
        "Vector" => Some(3),
        "Rotator" => Some(3),
        "Vector2D" => Some(2),
        "LinearColor" => Some(4),
        _ => None,
    }
}

fn stream(b: &[u8], mut p: usize, limit: usize, prefix: &str, save: &Save, out: &mut Vec<Prop>) {
    while p + 4 <= limit {
        let nidx = rd_i32(b, p);
        let pname = save.name(nidx);
        if pname == "None" {
            break;
        }
        p += 4;
        if p + 4 > limit {
            break;
        }
        let ptype = save.name(rd_i32(b, p));
        p += 4;
        let size = rd_i64(b, p) as usize;
        p += 8;
        let full = if prefix.is_empty() {
            pname.clone()
        } else {
            format!("{}{}", prefix, pname)
        };
        p = value(b, p, size, &ptype, &full, save, out);
    }
}

fn value(
    b: &[u8],
    mut p: usize,
    size: usize,
    ptype: &str,
    path: &str,
    save: &Save,
    out: &mut Vec<Prop>,
) -> usize {
    match ptype {
        "BoolProperty" => {
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 2
        }
        "EnumProperty" => {
            p += 4;
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 4
        }
        "ByteProperty" => {
            p += 4;
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + if size == 1 { 1 } else { 4 }
        }
        "IntProperty" => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 4
        }
        "UInt32Property" => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 4
        }
        "FloatProperty" => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 4
        }
        "NameProperty" => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + 4
        }
        "ObjectProperty" => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + size
        }
        "StructProperty" => {
            let stype = save.name(rd_i32(b, p));
            p += 4;
            p += 16; // struct guid
            p += 1; // guid flag
            let body = p;
            if native_floats(&stype).is_some() {
                out.push(Prop {
                    path: path.into(),
                    ptype: format!("Struct:{}", stype),
                    voff: body,
                });
            } else {
                let pre = format!("{}.", path);
                stream(b, body, body + size, &pre, save, out);
            }
            body + size
        }
        "ArrayProperty" => {
            p += 4;
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + size
        }
        _ => {
            p += 1;
            out.push(Prop {
                path: path.into(),
                ptype: ptype.into(),
                voff: p,
            });
            p + size
        }
    }
}

fn find_voff(props: &[Prop], path: &str) -> Option<usize> {
    props.iter().find(|pr| pr.path == path).map(|pr| pr.voff)
}

fn is_shuttle(t: &str) -> bool {
    t.contains("Shuttle_T") && t.contains("ThrusterSlot")
}

fn is_pad(t: &str) -> bool {
    t.contains("LandingPad") || t.contains("ShuttlePlatform")
}

fn shuttle_is_lost(save: &Save, obj_index: usize) -> bool {
    if let Some(auto) = save.comp_obj(obj_index, "ShuttleAutomation") {
        let o = &save.objects[auto];
        let blob = &save.raw[o.data_off..o.data_off + o.data_len];
        let props = decode_props(blob, save);
        if let Some(v) = find_voff(&props, "bIsShuttleLost") {
            return blob[v] != 0;
        }
    }
    false
}

fn nearest_planet_center(save: &Save, pos: [f32; 3]) -> [f32; 3] {
    let mut best = [0.0f32; 3];
    let mut bd = f32::MAX;
    for (i, o) in save.objects.iter().enumerate() {
        if clean(&o.object_type).contains("T2_Planet") {
            if let Some(t) = save.trans(i) {
                let d = dist2(t, pos);
                if d < bd {
                    bd = d;
                    best = t;
                }
            }
        }
    }
    best
}
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

// nearest planet -> readable Astroneer planet name
fn planet_name(save: &Save, pos: [f32; 3]) -> String {
    let mut best = String::new();
    let mut bd = f32::MAX;
    for (i, o) in save.objects.iter().enumerate() {
        if clean(&o.object_type).contains("T2_Planet") {
            if let Some(t) = save.trans(i) {
                let d = dist2(t, pos);
                if d < bd {
                    bd = d;
                    best = save.name(o.name_index);
                }
            }
        }
    }
    let key = best.split('_').nth(2).unwrap_or("");
    match key {
        "Terran" => "Sylva",
        "TerranMoon" => "Desolo",
        "Arid" => "Calidor",
        "Exotic" => "Vesania",
        "ExoticMoon" => "Novus",
        "Tundra" => "Glacio",
        "Radiated" => "Atrox",
        other => other,
    }
    .to_string()
}

fn ship_label(short: &str) -> String {
    if short.contains("Shuttle_T2") {
        "Small Shuttle".into()
    } else if short.contains("Shuttle_T3") {
        "Medium Shuttle".into()
    } else if short.contains("Shuttle_T4") {
        "Large Shuttle".into()
    } else {
        prettify(short)
    }
}

// readable label from an object's short type ("Thruster_Medium.Thruster_Medium_C")
fn prettify(short: &str) -> String {
    let base = short.split('.').next().unwrap_or(short);
    match base {
        "Thruster_Medium_Consumable" => return "Solid-Fuel Thruster".into(),
        "Thruster_Medium" => return "Hydrazine Thruster".into(),
        "BuiltInVehicleSeat" => return "Cockpit seat".into(),
        "Seat_Medium" => return "Medium Seat".into(),
        "Seat3_Large" => return "Large Seat".into(),
        "DeployableLandingPad" => return "Deployable Landing Pad".into(),
        "NaturalLandingPad" => return "Natural Landing Spot".into(),
        "StationLandingPad" => return "Landing Zone (POI)".into(),
        "Item_ModernPOI_ShuttlePlatform" => return "Shuttle Platform (POI)".into(),
        _ => {}
    }
    let chars: Vec<char> = base.chars().collect();
    let mut s = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            s.push(' ');
            continue;
        }
        if ch.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            if prev != '_' && !prev.is_uppercase() {
                s.push(' ');
            }
        }
        s.push(ch);
    }
    s
}

// items slotted on a vehicle (thrusters, seats, storage...) read from its SlotsComponent graph
fn slot_items(save: &Save, ship_i: usize) -> Vec<String> {
    const ALLOW: [&str; 10] = [
        "Thruster", "Seat", "Storage", "Canister", "Battery", "Generator", "Oxygen", "Drill",
        "Crane", "Paver",
    ];
    let mut out = Vec::new();
    let mut seen = Vec::new();
    if let Some(sc) = save.comp_obj(ship_i, "SlotsComponent") {
        let o = &save.objects[sc];
        let mut d = save.raw[o.data_off..o.data_off + o.data_len].to_vec();
        d.extend_from_slice(&save.raw[o.custom_off..o.custom_off + o.custom_len]);
        let mut off = 0;
        while off + 4 <= d.len() {
            let v = u32::from_le_bytes(d[off..off + 4].try_into().unwrap()) as usize;
            if v > 0 && v < save.objects.len() && !seen.contains(&v) {
                let t = clean(&save.objects[v].object_type);
                let short = t.rsplit('/').next().unwrap_or(t);
                if ALLOW.iter().any(|k| short.contains(k)) {
                    seen.push(v);
                    out.push(prettify(short));
                }
            }
            off += 1;
        }
    }
    out
}

fn jstr(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn decode_json(blob: &[u8]) -> String {
    let save = match parse(blob) {
        Ok(s) => s,
        Err(e) => return format!("{{\"error\":{}}}", jstr(&e)),
    };
    let mut ships = Vec::new();
    let mut pads = Vec::new();
    for (i, o) in save.objects.iter().enumerate() {
        let t = clean(&o.object_type).to_string();
        if save.actor_of(i).is_none() {
            continue;
        }
        let pos = match save.trans(i) {
            Some(p) => p,
            None => continue,
        };
        let nm = save.name(o.name_index);
        let short = t.rsplit('/').next().unwrap_or(&t);
        if is_shuttle(&t) {
            let lost = shuttle_is_lost(&save, i);
            let slots: Vec<String> = slot_items(&save, i).iter().map(|s| jstr(s)).collect();
            ships.push(format!(
                "{{\"name\":{},\"type\":{},\"label\":{},\"planet\":{},\"lost\":{},\"pos\":[{:.0},{:.0},{:.0}],\"slots\":[{}]}}",
                jstr(&nm),
                jstr(short),
                jstr(&ship_label(short)),
                jstr(&planet_name(&save, pos)),
                lost,
                pos[0], pos[1], pos[2],
                slots.join(",")
            ));
        } else if is_pad(&t) {
            pads.push(format!(
                "{{\"name\":{},\"type\":{},\"label\":{},\"planet\":{},\"pos\":[{:.0},{:.0},{:.0}]}}",
                jstr(&nm),
                jstr(short),
                jstr(&prettify(short)),
                jstr(&planet_name(&save, pos)),
                pos[0], pos[1], pos[2]
            ));
        }
    }
    let host = match save.host_pawn.and_then(|p| save.trans(p as usize)) {
        Some(p) => format!(
            "{{\"pos\":[{:.0},{:.0},{:.0}],\"planet\":{}}}",
            p[0], p[1], p[2], jstr(&planet_name(&save, p))
        ),
        None => "null".into(),
    };
    format!(
        "{{\"ships\":[{}],\"pads\":[{}],\"host\":{}}}",
        ships.join(","),
        pads.join(","),
        host
    )
}

fn obj_by_name(save: &Save, name: &str) -> Option<usize> {
    save.objects
        .iter()
        .position(|o| save.name(o.name_index) == name)
}

fn write_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
fn write_f32(buf: &mut [u8], off: usize, v: f32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub fn force_land(blob: &[u8], ship_name: &str, pad_name: &str) -> Result<Vec<u8>, String> {
    let save = parse(blob)?;
    let ship_i = obj_by_name(&save, ship_name).ok_or("ship not found")?;
    let pad_i = obj_by_name(&save, pad_name).ok_or("pad not found")?;
    if !is_shuttle(clean(&save.objects[ship_i].object_type)) {
        return Err("selected object is not a shuttle".into());
    }

    let pad_pos = save.trans(pad_i).ok_or("pad has no transform")?;
    let planet = nearest_planet_center(&save, pad_pos);

    let mut rv = [
        pad_pos[0] - planet[0],
        pad_pos[1] - planet[1],
        pad_pos[2] - planet[2],
    ];

    let rn = (rv[0] * rv[0] + rv[1] * rv[1] + rv[2] * rv[2]).sqrt();
    if rn > 0.0 {
        for k in 0..3 {
            rv[k] /= rn;
        }
    }
    let new_pos = [
        pad_pos[0] + rv[0] * RAISE,
        pad_pos[1] + rv[1] * RAISE,
        pad_pos[2] + rv[2] * RAISE,
    ];

    let mut ref_i: Option<usize> = None;
    let mut ref_d = f32::MAX;
    for (i, o) in save.objects.iter().enumerate() {
        if i == ship_i || !is_shuttle(clean(&o.object_type)) {
            continue;
        }
        if shuttle_is_lost(&save, i) {
            continue;
        }
        if let Some(t) = save.trans(i) {
            // same planet => nearest planet center matches
            if dist2(nearest_planet_center(&save, t), planet) < 1.0 {
                let d = dist2(t, pad_pos);
                if d < ref_d {
                    ref_d = d;
                    ref_i = Some(i);
                }
            }
        }
    }

    let good_rot: [f32; 4] = {
        let src = ref_i.unwrap_or(pad_i);
        let a = save.actor_of(src).unwrap();
        [
            save.read_f32(a.rot_off),
            save.read_f32(a.rot_off + 4),
            save.read_f32(a.rot_off + 8),
            save.read_f32(a.rot_off + 12),
        ]
    };

    // gather offsets we need from the ship's nav + auto, and (optionally) the reference's
    let nav_o = save
        .comp_obj(ship_i, "OrbitalNavigation")
        .ok_or("ship has no OrbitalNavigation")?;
    let auto_o = save
        .comp_obj(ship_i, "ShuttleAutomation")
        .ok_or("ship has no ShuttleAutomation")?;
    let nav_base = save.objects[nav_o].data_off;
    let auto_base = save.objects[auto_o].data_off;
    let nav_props = decode_props(
        &save.raw[nav_base..nav_base + save.objects[nav_o].data_len],
        &save,
    );
    let auto_props = decode_props(
        &save.raw[auto_base..auto_base + save.objects[auto_o].data_len],
        &save,
    );

    // reference nav/auto props (for copying landed transform / enum / source)
    let ref_data = ref_i.map(|ri| {
        let rn_o = save.comp_obj(ri, "OrbitalNavigation").unwrap();
        let ra_o = save.comp_obj(ri, "ShuttleAutomation").unwrap();
        let rnb = save.objects[rn_o].data_off;
        let rab = save.objects[ra_o].data_off;
        let rnp = decode_props(&save.raw[rnb..rnb + save.objects[rn_o].data_len], &save);
        let rap = decode_props(&save.raw[rab..rab + save.objects[ra_o].data_len], &save);
        (rnb, rnp, rab, rap)
    });

    let mut raw = save.raw.clone();

    // 1) ShuttleAutomation: bIsShuttleLost = False
    if let Some(v) = find_voff(&auto_props, "bIsShuttleLost") {
        raw[auto_base + v] = 0;
    }
    // ShuttleSequenceState = Landed (copy ref's enum value, else look up the table index)
    if let Some(v) = find_voff(&auto_props, "ShuttleSequenceState") {
        let landed_idx = if let Some((_, _, rab, rap)) = &ref_data {
            find_voff(rap, "ShuttleSequenceState").map(|rv| rd_i32(&save.raw[*rab + rv..], 0))
        } else {
            None
        }
        .or_else(|| save.find_string_index("EShuttleSequenceState::Landed"));
        if let Some(idx) = landed_idx {
            write_i32(&mut raw, auto_base + v, idx);
        }
    }

    // 2) OrbitalNavigation
    for path in [
        "ReplicatedData.CurrentOrbitingBody",
        "ReplicatedData.TargetOrbitingBody",
        "ReplicatedData.CurrentComponent",
        "ReplicatedData.TargetComponent",
    ] {
        if let Some(v) = find_voff(&nav_props, path) {
            write_i32(&mut raw, nav_base + v, -1);
        }
    }

    // SourceOrbitingBody <- ref's value (if available)
    if let (Some(v), Some((rnb, rnp, _, _))) = (
        find_voff(&nav_props, "ReplicatedData.SourceOrbitingBody"),
        &ref_data,
    ) {
        if let Some(rv) = find_voff(rnp, "ReplicatedData.SourceOrbitingBody") {
            let val = rd_i32(&save.raw[*rnb + rv..], 0);
            write_i32(&mut raw, nav_base + v, val);
        }
    }

    // Current/Target Transform <- ref landed transform (rotation 4f + translation 3f)
    if let Some((rnb, rnp, _, _)) = &ref_data {
        for base in [
            "ReplicatedData.CurrentTransform",
            "ReplicatedData.TargetTransform",
        ] {
            for (sub, n) in [("Rotation", 4usize), ("Translation", 3usize)] {
                let path = format!("{}.{}", base, sub);
                if let (Some(dv), Some(sv)) = (find_voff(&nav_props, &path), find_voff(rnp, &path))
                {
                    for k in 0..n {
                        let f = save.read_f32(*rnb + sv + 4 * k);
                        write_f32(&mut raw, nav_base + dv + 4 * k, f);
                    }
                }
            }
        }
    }

    // Current/Target ComponentTransform <- new pad transform (rotation = good_rot, translation = new_pos)
    for base in [
        "ReplicatedData.CurrentComponentTransform",
        "ReplicatedData.TargetComponentTransform",
    ] {
        if let Some(rv) = find_voff(&nav_props, &format!("{}.Rotation", base)) {
            for k in 0..4 {
                write_f32(&mut raw, nav_base + rv + 4 * k, good_rot[k]);
            }
        }
        if let Some(tv) = find_voff(&nav_props, &format!("{}.Translation", base)) {
            for k in 0..3 {
                write_f32(&mut raw, nav_base + tv + 4 * k, new_pos[k]);
            }
        }
    }

    // 3) actor root transform of the ship
    {
        let a = save.actor_of(ship_i).unwrap();
        for k in 0..4 {
            write_f32(&mut raw, a.rot_off + 4 * k, good_rot[k]);
        }
        for k in 0..3 {
            write_f32(&mut raw, a.trans_off + 4 * k, new_pos[k]);
        }
    }

    // 4) ExitSuppressionCount = 0 on every seat attached to this ship
    for si in ship_seats(&save, ship_i) {
        if let Some(aac) = save.comp_obj(si, "ActorAttachments") {
            let o = &save.objects[aac];
            let base = o.data_off;
            let props = decode_props(&save.raw[base..base + o.data_len], &save);
            if let Some(v) = find_voff(&props, "ExitSuppressionCount") {
                write_i32(&mut raw, base + v, 0);
            }
        }
    }

    Ok(repack(&save.header, &raw))
}

fn ship_seats(save: &Save, ship_i: usize) -> Vec<usize> {
    let mut seats = Vec::new();
    if let Some(sc) = save.comp_obj(ship_i, "SlotsComponent") {
        let o = &save.objects[sc];
        let mut d = save.raw[o.data_off..o.data_off + o.data_len].to_vec();
        d.extend_from_slice(&save.raw[o.custom_off..o.custom_off + o.custom_len]);
        let mut off = 0;
        while off + 4 <= d.len() {
            let v = u32::from_le_bytes(d[off..off + 4].try_into().unwrap());
            let idx = (v & 0x0FFF_FFFF) as usize;
            if idx > 0
                && idx < save.objects.len()
                && clean(&save.objects[idx].object_type).contains("Seat")
                && !seats.contains(&idx)
            {
                seats.push(idx);
            }
            off += 1;
        }
    }
    seats
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn repack(orig_header: &[u8], raw: &[u8]) -> Vec<u8> {
    // zlib stream with CINFO=4 (4KB window) -> header byte 0x48; FLG 0x89 (matches the proven good save)
    let mut z = vec![0x48u8, 0x89u8];
    // DEFLATE stored blocks (BTYPE=00), <=65535 bytes each
    let mut i = 0usize;
    while i < raw.len() {
        let n = (raw.len() - i).min(65535);
        let bfinal = if i + n >= raw.len() { 1u8 } else { 0u8 };
        z.push(bfinal); // BTYPE=00
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&raw[i..i + n]);
        i += n;
    }
    if raw.is_empty() {
        z.push(1);
        z.extend_from_slice(&[0, 0, 0xff, 0xff]);
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());

    // outer 16-byte header: preserve 8-byte digest + version, set uncompressed size
    let mut out = orig_header[..12].to_vec();
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&z);
    out
}

// ----------------------------- WASM bindings -----------------------------
#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    pub fn decode(data: &[u8]) -> String {
        super::decode_json(data)
    }
    #[wasm_bindgen]
    pub fn force_land(data: &[u8], ship: &str, pad: &str) -> Result<Vec<u8>, JsValue> {
        super::force_land(data, ship, pad).map_err(|e| JsValue::from_str(&e))
    }
}
