use keyring::Entry;

const SERVICE: &str = "gitview";
const USER: &str = "github";

pub fn load() -> Option<String> {
	Entry::new(SERVICE, USER).ok()?.get_password().ok()
}

pub fn save(token: &str) -> keyring::Result<()> {
	Entry::new(SERVICE, USER)?.set_password(token)
}

pub fn delete() {
	if let Ok(entry) = Entry::new(SERVICE, USER) {
		let _ = entry.delete_credential();
	}
}
