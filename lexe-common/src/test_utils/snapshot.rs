/// Parse out the sample data json lines `Ok(line)` and comments `Err(line)`
pub fn parse_sample_data(data: &str) -> impl Iterator<Item = &str> {
    let data = data.trim();
    data.lines().filter(|line| {
        if line.is_empty() {
            return false;
        }

        // Print data and comments for easier debugging when something breaks
        println!("{line}");

        // Ignore comments
        !line.starts_with("---")
    })
}
