use std::{
    borrow::{BorrowMut, Cow},
    cell::RefMut,
    collections::HashMap,
};

use html_escape::decode_html_entities;
use lol_html::html_content::{Comment, ContentType, Doctype, Element, EndTag};
use magnus::{
    class, exception, function, method, scan_args, Error, Module, Object, RArray, RHash, RModule,
    Value,
};

use crate::tags::Tag;

#[derive(Clone, Debug)]
struct ElementSanitizer {
    allowed_attrs: Vec<String>,
    required_attrs: Vec<String>,
    allowed_classes: Vec<String>,
    protocol_sanitizers: HashMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Sanitizer {
    flags: [u8; Tag::TAG_COUNT],
    allowed_attrs: Vec<String>,
    allowed_classes: Vec<String>,
    element_sanitizers: HashMap<String, ElementSanitizer>,
    name_prefix: String,
    pub allow_comments: bool,
    pub allow_doctype: bool,
    config: RHash,
}

#[derive(Clone, Debug)]
#[magnus::wrap(class = "Selma::Sanitizer")]
pub struct SelmaSanitizer(std::cell::RefCell<Sanitizer>);

impl SelmaSanitizer {
    const SELMA_SANITIZER_ALLOW: u8 = (1 << 0);
    const SELMA_SANITIZER_REMOVE_CONTENTS: u8 = (1 << 1);
    const SELMA_SANITIZER_WRAP_WHITESPACE: u8 = (1 << 2);

    pub fn new(arguments: &[Value]) -> Result<Self, Error> {
        let args = scan_args::scan_args::<(), (Option<RHash>,), (), (), (), ()>(arguments)?;
        let (opt_config,): (Option<RHash>,) = args.optional;

        let config = match opt_config {
            Some(config) => config,
            // TODO: this seems like a hack to fix?
            None => magnus::eval::<RHash>(r#"Selma::Sanitizer::Config::DEFAULT"#).unwrap(),
        };

        let mut element_sanitizers = HashMap::new();
        Tag::html_tags().iter().for_each(|html_tag| {
            let es = ElementSanitizer {
                allowed_attrs: vec![],
                allowed_classes: vec![],
                required_attrs: vec![],

                protocol_sanitizers: HashMap::new(),
            };
            element_sanitizers.insert(Tag::element_name_from_enum(html_tag).to_string(), es);
        });

        Ok(Self(std::cell::RefCell::new(Sanitizer {
            flags: [0; Tag::TAG_COUNT],
            allowed_attrs: vec![],
            allowed_classes: vec![],
            element_sanitizers,
            name_prefix: "".to_string(),
            allow_comments: false,
            allow_doctype: false,
            config,
        })))
    }

    fn config(&self) -> RHash {
        self.0.borrow().config
    }

    /// Toggle a sanitizer option on or off.
    fn set_flag(&self, element: String, flag: u8, set: bool) {
        let tag = Tag::tag_from_element_name(&element);
        if set {
            self.0.borrow_mut().flags[tag.index] |= flag;
        } else {
            self.0.borrow_mut().flags[tag.index] &= !flag;
        }
    }

    /// Toggles all sanitization options on or off.
    fn set_all_flags(&self, flag: u8, set: bool) {
        if set {
            Tag::html_tags().iter().enumerate().for_each(|(iter, _)| {
                self.0.borrow_mut().flags[iter] |= flag;
            });
        } else {
            Tag::html_tags().iter().enumerate().for_each(|(iter, _)| {
                self.0.borrow_mut().flags[iter] &= flag;
            });
        }
    }

    /// Whether or not to keep HTML comments.
    fn set_allow_comments(&self, allow: bool) -> bool {
        self.0.borrow_mut().allow_comments = allow;
        allow
    }

    pub fn sanitize_comment(&self, c: &mut Comment) {
        if !self.0.borrow().allow_comments {
            c.remove();
        }
    }

    /// Whether or not to keep HTML doctype.
    fn set_allow_doctype(&self, allow: bool) -> bool {
        self.0.borrow_mut().allow_doctype = allow;
        allow
    }

    pub fn sanitize_doctype(&self, d: &mut Doctype) {
        if !self.0.borrow().allow_doctype {
            d.remove();
        }
    }

    fn set_allowed_attribute(&self, eln: Value, attr_name: String, allow: bool) -> bool {
        let mut binding = self.0.borrow_mut();

        let element_name = eln.to_r_string().unwrap().to_string().unwrap();
        if element_name == "all" {
            let allowed_attrs = &mut binding.allowed_attrs;
            Self::set_allowed(allowed_attrs, &attr_name, allow);
        } else {
            let element_sanitizer = Self::get_mut_element_sanitizer(&mut binding, &element_name);

            element_sanitizer.allowed_attrs.push(attr_name);
        }

        allow
    }

    fn set_allowed_class(&self, element_name: String, class_name: String, allow: bool) -> bool {
        let mut binding = self.0.borrow_mut();
        if element_name == "all" {
            let allowed_classes = &mut binding.allowed_classes;
            Self::set_allowed(allowed_classes, &class_name, allow);
        } else {
            let element_sanitizer = Self::get_mut_element_sanitizer(&mut binding, &element_name);

            let allowed_classes = element_sanitizer.allowed_classes.borrow_mut();
            Self::set_allowed(allowed_classes, &class_name, allow)
        }
        allow
    }

    fn set_allowed_protocols(&self, element_name: String, attr_name: String, allow_list: RArray) {
        let mut binding = self.0.borrow_mut();

        let element_sanitizer = Self::get_mut_element_sanitizer(&mut binding, &element_name);

        let protocol_sanitizers = element_sanitizer.protocol_sanitizers.borrow_mut();

        for opt_allowed_protocol in allow_list.each() {
            let allowed_protocol = opt_allowed_protocol.unwrap();
            let protocol_list = protocol_sanitizers.get_mut(&attr_name);
            if allowed_protocol.is_kind_of(class::string()) {
                match protocol_list {
                    None => {
                        protocol_sanitizers
                            .insert(attr_name.to_string(), vec![allowed_protocol.to_string()]);
                    }
                    Some(protocol_list) => protocol_list.push(allowed_protocol.to_string()),
                }
            } else if allowed_protocol.is_kind_of(class::symbol())
                && allowed_protocol.inspect() == ":relative"
            {
                match protocol_list {
                    None => {
                        protocol_sanitizers.insert(
                            attr_name.to_string(),
                            vec!["#".to_string(), "/".to_string()],
                        );
                    }
                    Some(protocol_list) => {
                        protocol_list.push("#".to_string());
                        protocol_list.push("/".to_string());
                    }
                }
            }
        }
    }

    fn set_allowed(set: &mut Vec<String>, attr_name: &String, allow: bool) {
        if allow {
            set.push(attr_name.to_string());
        } else if set.contains(attr_name) {
            set.swap_remove(set.iter().position(|x| x == attr_name).unwrap());
        }
    }

    pub fn sanitize_attributes(&self, element: &mut Element) {
        let keep_element: bool = Self::try_remove_element(self, element);

        if keep_element {
            return;
        }

        let binding = self.0.borrow_mut();
        let tag = Tag::tag_from_element_name(&element.tag_name().to_lowercase());
        let element_sanitizer = Self::get_element_sanitizer(&binding, &element.tag_name());

        // FIXME: This is a hack to get around the fact that we can't borrow
        let attribute_map: HashMap<String, String> = element
            .attributes()
            .iter()
            .map(|a| (a.name(), a.value()))
            .collect();

        for (attr_name, attr_val) in attribute_map.iter() {
            // you can actually embed <!-- ... --> inside
            // an HTML tag to pass malicious data. If this is
            // encountered, remove the entire element to be safe.
            if attr_name.starts_with("<!--") {
                let tag = Tag::tag_from_element_name(&element.tag_name().to_lowercase());
                let flags: u8 = binding.flags[tag.index];

                Self::force_remove_element(self, element, tag, flags);
                return;
            }

            if !attr_val.is_empty() {
                // first, trim leading spaces and unescape any encodings
                let trimmed = attr_val.trim_start();
                let x = escapist::unescape_html(trimmed.as_bytes());
                let unescaped_attr_val = String::from_utf8_lossy(&x).to_string();

                // element.set_attribute(attr_name, &decoded_attribute);

                if !Self::should_keep_attribute(
                    &binding,
                    element,
                    element_sanitizer,
                    attr_name,
                    &unescaped_attr_val,
                ) {
                    element.remove_attribute(attr_name);
                } else {
                    // Prevent the use of `<meta>` elements that set a charset other than UTF-8,
                    // since output is always UTF-8.
                    if Tag::is_meta(tag) {
                        if attr_name == "charset" && unescaped_attr_val != "utf-8" {
                            element.set_attribute(attr_name, "utf-8");
                        }
                    } else {
                        let mut buf = String::new();
                        // ...then, escape any special characters, for security
                        if attr_name == "href" { // FIXME: gross--------------vvvv
                            escapist::escape_href(&mut buf, unescaped_attr_val.to_string().as_str());
                        } else {
                            escapist::escape_html(&mut buf, unescaped_attr_val.to_string().as_str());
                        };

                        element.set_attribute(attr_name, &buf);
                    }
                }
            } else {
                // no value? remove the attribute
                element.remove_attribute(attr_name);
            }
        }

        let required = &element_sanitizer.required_attrs;
        if required.contains(&"*".to_string()) {
            return;
        }
        for attr in element.attributes().iter() {
            let attr_name = &attr.name();
            if required.contains(attr_name) {
                return;
            }
        }
    }

    fn should_keep_attribute(
        binding: &RefMut<Sanitizer>,
        element: &mut Element,
        element_sanitizer: &ElementSanitizer,
        attr_name: &String,
        attr_val: &String,
    ) -> bool {
        let mut allowed = element_sanitizer.allowed_attrs.contains(attr_name);

        if !allowed && binding.allowed_attrs.contains(attr_name) {
            allowed = true;
        }

        if !allowed {
            return false;
        }

        let protocol_sanitizer_values = element_sanitizer.protocol_sanitizers.get(attr_name);
        match protocol_sanitizer_values {
            None => {}
            Some(protocol_sanitizer_values) => {
                if !Self::has_allowed_protocol(protocol_sanitizer_values, attr_val) {
                    return false;
                }
            }
        }

        if attr_name == "class"
            && !Self::sanitize_class_attribute(
                binding,
                element,
                element_sanitizer,
                attr_name,
                attr_val,
            )
            .unwrap()
        {
            return false;
        }

        true
    }

    fn has_allowed_protocol(protocols_allowed: &Vec<String>, attr_val: &String) -> bool {
        // FIXME: is there a more idiomatic way to do this?
        let mut pos: usize = 0;
        let mut chars = attr_val.chars();
        let len = attr_val.len();

        for (i, c) in attr_val.chars().enumerate() {
            if c != ':' && c != '/' && c != '#' && pos + 1 < len {
                pos = i + 1;
            } else {
                break;
            }
        }

        let char = chars.nth(pos).unwrap();

        if char == '/' {
            return protocols_allowed.contains(&"/".to_string());
        }

        if char == '#' {
            return protocols_allowed.contains(&"#".to_string());
        }

        // Allow protocol name to be case-insensitive
        let protocol = attr_val[0..pos].to_lowercase();
        protocols_allowed.contains(&protocol.to_lowercase())
    }

    fn sanitize_class_attribute(
        binding: &RefMut<Sanitizer>,
        element: &mut Element,
        element_sanitizer: &ElementSanitizer,
        attr_name: &str,
        attr_val: &str,
    ) -> Result<bool, Error> {
        let allowed_global = &binding.allowed_classes;

        let mut valid_classes: Vec<String> = vec![];

        let allowed_local = &element_sanitizer.allowed_classes;

        // No class filters, so everything goes through
        if allowed_global.is_empty() && allowed_local.is_empty() {
            return Ok(true);
        }

        let attr_value = attr_val.trim_start();
        attr_value
            .split_whitespace()
            .map(|s| s.to_string())
            .for_each(|class| {
                if allowed_global.contains(&class) || allowed_local.contains(&class) {
                    valid_classes.push(class);
                }
            });

        if valid_classes.is_empty() {
            return Ok(false);
        }

        match element.set_attribute(attr_name, valid_classes.join(" ").as_str()) {
            Ok(_) => Ok(true),
            Err(err) => Err(Error::new(
                exception::runtime_error(),
                format!("AttributeNameError: {}", err),
            )),
        }
    }

    pub fn try_remove_element(&self, element: &mut Element) -> bool {
        let tag = Tag::tag_from_element_name(&element.tag_name().to_lowercase());
        let flags: u8 = self.0.borrow().flags[tag.index];

        let should_remove: bool = (flags & Self::SELMA_SANITIZER_ALLOW) == 0;

        if should_remove {
            if Tag::has_text_content(tag) {
                Self::remove_element(element, tag, Self::SELMA_SANITIZER_REMOVE_CONTENTS);
            } else {
                Self::remove_element(element, tag, flags);
            }

            Self::check_if_end_tag_needs_removal(element);
        } else {
            // anything in <iframe> must be removed, if it's kept
            if Tag::is_iframe(tag) {
                if self.0.borrow().flags[tag.index] != 0 {
                    element.set_inner_content(" ", ContentType::Text);
                } else {
                    element.set_inner_content("", ContentType::Text);
                }
            }
        }

        should_remove
    }

    fn remove_element(element: &mut Element, tag: Tag, flags: u8) {
        let wrap_whitespace = (flags & Self::SELMA_SANITIZER_WRAP_WHITESPACE) != 0;
        let remove_contents = (flags & Self::SELMA_SANITIZER_REMOVE_CONTENTS) != 0;

        if remove_contents {
            element.remove();
        } else {
            if wrap_whitespace {
                if tag.self_closing {
                    element.after(" ", ContentType::Text);
                } else {
                    element.before(" ", ContentType::Text);
                    element.after(" ", ContentType::Text);
                }
            }
            element.remove_and_keep_content();
        }
    }

    fn force_remove_element(&self, element: &mut Element, tag: Tag, flags: u8) {
        Self::remove_element(element, tag, flags);
        Self::check_if_end_tag_needs_removal(element);
    }

    fn check_if_end_tag_needs_removal(element: &mut Element) {
        if element.removed()
            && !Tag::tag_from_element_name(&element.tag_name().to_lowercase()).self_closing
        {
            element
                .on_end_tag(move |end| {
                    Self::remove_end_tag(end);
                    Ok(())
                })
                .unwrap();
        }
    }

    fn remove_end_tag(end_tag: &mut EndTag) {
        end_tag.remove();
    }

    fn get_element_sanitizer<'a>(
        binding: &'a RefMut<Sanitizer>,
        element_name: &str,
    ) -> &'a ElementSanitizer {
        binding.element_sanitizers.get(element_name).unwrap()
    }

    fn get_mut_element_sanitizer<'a>(
        binding: &'a mut Sanitizer,
        element_name: &str,
    ) -> &'a mut ElementSanitizer {
        binding.element_sanitizers.get_mut(element_name).unwrap()
    }
}

pub fn init(m_selma: RModule) -> Result<(), Error> {
    let c_sanitizer = m_selma.define_class("Sanitizer", Default::default())?;

    c_sanitizer.define_singleton_method("new", function!(SelmaSanitizer::new, -1))?;
    c_sanitizer.define_method("config", method!(SelmaSanitizer::config, 0))?;

    c_sanitizer.define_method("set_flag", method!(SelmaSanitizer::set_flag, 3))?;
    c_sanitizer.define_method("set_all_flags", method!(SelmaSanitizer::set_all_flags, 2))?;

    c_sanitizer.define_method(
        "set_allow_comments",
        method!(SelmaSanitizer::set_allow_comments, 1),
    )?;
    c_sanitizer.define_method(
        "set_allow_doctype",
        method!(SelmaSanitizer::set_allow_doctype, 1),
    )?;

    c_sanitizer.define_method(
        "set_allowed_attribute",
        method!(SelmaSanitizer::set_allowed_attribute, 3),
    )?;

    c_sanitizer.define_method(
        "set_allowed_class",
        method!(SelmaSanitizer::set_allowed_class, 3),
    )?;

    c_sanitizer.define_method(
        "set_allowed_protocols",
        method!(SelmaSanitizer::set_allowed_protocols, 3),
    )?;

    Ok(())
}
