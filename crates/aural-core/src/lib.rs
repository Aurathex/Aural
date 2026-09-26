pub const APP_ID: &str = "aural";

#[cfg(test)]
mod tests {
    #[test]
    fn app_id_is_lowercase_ascii() {
        assert!(super::APP_ID.chars().all(|c| c.is_ascii_lowercase()));
    }
}
