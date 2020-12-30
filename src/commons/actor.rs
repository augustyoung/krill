#[cfg(feature = "multi-user")]
use oso::ToPolar;
#[cfg(feature = "multi-user")]
use std::fmt::Display;

use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;

use crate::{commons::{KrillResult, error::Error}, constants::ACTOR_DEF_ANON, daemon::auth::Auth};
use crate::daemon::auth::policy::AuthPolicy;

#[derive(Clone, Eq, PartialEq)]
pub enum ActorName {
    AsStaticStr(&'static str),
    AsString(String),
}

impl ActorName {
    pub fn as_str(&self) -> &str {
        match &self {
            ActorName::AsStaticStr(s) => s,
            ActorName::AsString(s) => s,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Attributes {
    None,
    RoleOnly(&'static str),
    UserDefined(HashMap<String, String>)
}

impl Attributes {
    pub fn as_map(&self) -> HashMap<String, String> {
        match &self {
            Attributes::UserDefined(map) => map.clone(),
            Attributes::RoleOnly(role) => {
                let mut map = HashMap::new();
                map.insert("role".to_string(), role.to_string());
                map
            },
            Attributes::None => HashMap::new()
        }
    }
}

#[derive(Clone)]
pub struct ActorDef {
    pub name: ActorName,
    pub is_user: bool,
    pub attributes: Attributes,
    pub new_auth: Option<Auth>,
    pub auth_error: Option<String>,
}

impl ActorDef {
    // store an error string instead of an Error because Error cannot be cloned.
    pub fn with_auth_error(mut self, error_msg: String) -> Self {
        self.auth_error = Some(error_msg);
        self
    }
}

#[derive(Clone)]
pub struct Actor {
    name: ActorName,
    is_user: bool,
    attributes: Attributes,
    new_auth: Option<Auth>,
    policy: Option<AuthPolicy>,
    auth_error: Option<String>,
}

impl PartialEq for Actor {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name &&
        self.is_user == other.is_user &&
        self.attributes == other.attributes
    }
}

impl PartialEq<ActorDef> for Actor {
    fn eq(&self, other: &ActorDef) -> bool {
        self.name == other.name &&
        self.is_user == other.is_user &&
        self.attributes == other.attributes
    }
}

impl Actor {
    pub const fn anonymous() -> ActorDef {
        ActorDef {
            name: ActorName::AsStaticStr("anonymous"),
            is_user: false,
            attributes: Attributes::None,
            new_auth: None,
            auth_error: None,
        }
    }

    pub const fn system(name: &'static str, role: &'static str) -> ActorDef {
        ActorDef {
            name: ActorName::AsStaticStr(name),
            attributes: Attributes::RoleOnly(role),
            is_user: false,
            new_auth: None,
            auth_error: None,
        }
    }

    pub fn user(name: String, attributes: &HashMap<String, String>, new_auth: Option<Auth>) -> ActorDef {
        ActorDef {
            name: ActorName::AsString(name),
            is_user: true,
            attributes: Attributes::UserDefined(attributes.clone()),
            new_auth,
            auth_error: None,
        }
    }

    /// Only for use in testing
    pub fn test_from_def(repr: &ActorDef) -> Actor {
        Actor {
            name: repr.name.clone(),
            is_user: repr.is_user,
            attributes: repr.attributes.clone(),
            new_auth: None,
            auth_error: None,
            policy: None,
        }
    }

    /// Only for use in testing
    pub fn test_from_details(name: String, attrs: HashMap<String, String>) -> Actor {
        Actor {
            name: ActorName::AsString(name),
            attributes: Attributes::UserDefined(attrs),
            is_user: false,
            new_auth: None,
            auth_error: None,
            policy: None,
        }
    }

    pub fn new(repr: &ActorDef, policy: AuthPolicy) -> Actor {
        Actor {
            name: repr.name.clone(),
            is_user: repr.is_user,
            attributes: repr.attributes.clone(),
            new_auth: repr.new_auth.clone(),
            auth_error: repr.auth_error.clone(),
            policy: Some(policy),
        }
    }

    pub fn is_user(&self) -> bool {
        self.is_user
    }

    pub fn is_anonymous(&self) -> bool {
        self == ACTOR_DEF_ANON
    }

    pub fn new_auth(&self) -> Option<Auth> {
        self.new_auth.clone()
    }

    pub fn attributes(&self) -> HashMap<String, String> {
        self.attributes.as_map()
    }

    pub fn attribute(&self, attr_name: String) -> Option<String> {
        match &self.attributes {
            Attributes::UserDefined(map)                       => map.get(&attr_name).cloned(),
            Attributes::RoleOnly(role) if &attr_name == "role" => Some(role.to_string()),
            Attributes::RoleOnly(_)                            => None,
            Attributes::None                                   => None,
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    #[cfg(not(feature = "multi-user"))]
    pub fn is_allowed<A, R>(&self, _: A, _: R) -> KrillResult<bool> {
        Ok(true)
    }

    #[cfg(feature = "multi-user")]
    pub fn is_allowed<A, R>(&self, action: A, resource: R)
         -> KrillResult<bool>
    where
        A: ToPolar + Display + Clone,
        R: ToPolar + Display + Clone,
    {
        if let Some(error_msg) = &self.auth_error {
            trace!("Unable to check access: actor={}, action={}, resource={}: {}",
                self.name(), &action, &resource, &error_msg);
            return Err(Error::ApiInvalidCredentials(error_msg.clone()));
        }

        match &self.policy {
            Some(policy) => {
                match policy.is_allowed(self.clone(), action.clone(), resource.clone()) {
                    Ok(allowed) => {
                        if log_enabled!(log::Level::Trace) {
                            if allowed {
                                trace!("Access granted: actor={}, action={}, resource={}",
                                    self.name(), &action, &resource);
                            } else {
                                trace!("Access denied: actor={:?}, action={}, resource={}",
                                    self, &action, &resource);
                            }
                        }
                        Ok(allowed)
                    },
                    Err(err) => {
                        error!("Unable to check access: actor={}, action={}, resource={}: {}",
                            self.name(), &action, &resource, err);
                        Ok(false)
                    }
                }
            },
            None => {
                // Auth policy is required, can only be omitted for use by test
                // rules inside an Oso policy. We should never get here, but we
                // don't want to crash Krill by calling unreachable!().
                error!("Unable to check access: actor={}, action={}, resource={}: {}",
                    self.name(), &action, &resource, "Internal error: missing policy");
                Ok(false)
            }
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl fmt::Debug for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Actor(name={:?}, is_user={}, attr={:?})",
            self.name(), self.is_user, self.attributes)
    }
}