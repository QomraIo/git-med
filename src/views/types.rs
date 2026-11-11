pub struct ViewAttributes<'a> {
    pub subject: &'a str,
    pub study: &'a str,
    pub series: &'a str,
    pub instance: &'a str,

    pub source: &'a str,
    pub task: &'a str,
}

pub struct ViewAttributesIndex {
    pub subject: Option<[u8; 16]>,
    pub study: Option<[u8; 16]>,
    pub series: Option<[u8; 16]>,
    pub instance: Option<[u8; 16]>,

    pub source: Option<[u8; 16]>,
    pub task: Option<[u8; 16]>,
}
