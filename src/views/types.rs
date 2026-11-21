pub struct ViewAttributes<'a> {
    pub subject: &'a str,
    pub study: &'a str,
    pub series: &'a str,
    pub instance: &'a str,

    pub source: &'a str,
    pub task: &'a str,
}

pub struct ViewAttributesIndex {
    pub subject: Option<String>,
    pub study: Option<String>,
    pub series: Option<String>,
    pub instance: Option<String>,

    pub source: Option<String>,
    pub task: Option<String>,
}
