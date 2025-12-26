use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Room {
    pub meta: RoomMeta,
    pub narrative: Narrative,
    #[serde(rename = "puzzle")]
    pub challenge: Challenge,
    pub scoring: Option<Scoring>,
    #[serde(default)]
    pub rewards: Option<Rewards>,
    #[serde(default)]
    pub codex: Option<CodexEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CodexEntry {
    pub title: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct RoomMeta {
    pub id: String,
    pub room_number: u32,
    pub title: String,
    pub concept: String,
}

#[derive(Debug, Deserialize)]
pub struct Narrative {
    #[serde(default)]
    pub entry: Option<String>,  // Shown when entering room (transition from previous)
    pub intro: String,
    pub success: String,
    pub failure_compile: String,
    pub failure_output: String,
    pub hints: Vec<String>,
    #[serde(default)]
    pub alternative_solution: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Rewards {
    #[serde(default)]
    pub grants_item: Option<String>,
    #[serde(default)]
    pub item_description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Challenge {
    pub code: String,
    pub expected_output: String,
    #[serde(default)]
    pub locked_lines: Vec<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Scoring {
    pub par_time_seconds: Option<u32>,
    pub hint_penalty_hp: Option<u32>,
    pub wrong_answer_penalty_hp: Option<u32>,
}
