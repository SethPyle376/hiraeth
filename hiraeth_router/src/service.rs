pub enum AuthRequirement {
    RequiredSigV4,
    OptionalSigV4,
    AnonymousAllowed,
    Disabled,
}

pub trait Service {
    fn auth_requirement(&self) -> AuthRequirement;
}
