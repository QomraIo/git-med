pub struct ViewAttributes<'a> {
    pub subject: &'a str,
    pub study: &'a str,
    pub series: &'a str,
    pub instance: &'a str,

    pub source: &'a str,
    pub task: &'a str,
}
