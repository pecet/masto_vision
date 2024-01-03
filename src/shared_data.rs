use once_cell::sync::Lazy;

use std::{collections::HashSet, fs::File, io::Read, sync::Mutex};

pub struct SharedData {
    pub already_parsed: HashSet<String>,
}

impl Default for SharedData {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedData {
    pub fn new() -> Self {
        let file = File::open("already_parsed.json");
        let already_parsed: HashSet<String> = match file {
            Ok(mut file) => {
                let mut contents = String::new();
                file.read_to_string(&mut contents).unwrap();
                serde_json::from_str(&contents).unwrap()
            }
            Err(_) => HashSet::new(),
        };
        Self { already_parsed }
    }

    pub fn save(&self) {
        let file = File::create("already_parsed.json").unwrap();
        serde_json::to_writer(file, &self.already_parsed).unwrap();
    }
}

pub static SHARED_DATA: Lazy<Mutex<SharedData>> = Lazy::new(|| Mutex::new(SharedData::new()));

pub fn get_shared_data() -> &'static Mutex<SharedData> {
    &SHARED_DATA
}
