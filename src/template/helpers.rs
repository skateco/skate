use handlebars::{handlebars_helper, Handlebars, JsonRender};
use serde_json::Value;

pub fn new<'reg>() -> Handlebars<'reg> {
    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_helper("join", Box::new(join));
    handlebars.register_helper("default", Box::new(default));
    handlebars
}

handlebars_helper!(join: |{sep:str=","}, *args|
                   args.iter().map(|a| a.render()).collect::<Vec<String>>().join(sep)
);

// returns the default if the value is empty
handlebars_helper!(default: |value: Value, def: Value|
    if value.is_null() { def } else { value }
);

