use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ApiProblem {
    pub id: String,
    pub contest_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContestProblem {
    pub contest_id: String,
    pub problem_id: String,
    pub problem_index: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiContest {
    pub id: String,
    pub start_epoch_second: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProblemModel {
    pub difficulty: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Submission {
    pub result: String,
    pub problem_id: String,
    pub epoch_second: i64,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub contest_id: String,
    pub problem_index: String,
    pub title: String,
    pub difficulty: i64,
    pub raw_difficulty: f64,
    pub contest_start_epoch_second: i64,
    pub contest_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestFile {
    pub date: String,
    pub problems: Vec<ContestProblemFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContestProblemFile {
    pub slot: String,
    pub problem_id: String,
    pub contest_id: String,
    pub problem_index: String,
    pub title: String,
    pub difficulty: i64,
    pub raw_difficulty: f64,
    pub url: String,
    pub submit_url: String,
}
