from pathlib import Path

path = Path("src/web_runtime.rs")
text = path.read_text(encoding="utf-8")

old_import = "use crate::cmd::ChipCmd;\nuse crate::soc;"
new_import = "use crate::cmd::ChipCmd;\nuse crate::controller;\nuse crate::controller::saturn::{INPUT_TERMINALS, OUTPUT_TERMINALS};\nuse crate::soc;"
assert old_import in text, "web runtime imports changed"
text = text.replace(old_import, new_import, 1)

old_worlds = '''    let worlds = World::list()\n        .iter()\n        .map(|(id, description)| json!({ "id": id, "description": description }))\n        .collect::<Vec<_>>();\n\n    json!({\n        "boards": boards,\n        "socs": socs,\n        "pins": {\n            "phy6252": phy6252_pins,\n        },\n        "worlds": worlds,\n    })'''
new_worlds = '''    let controllers = controller::PROFILES\n        .iter()\n        .map(|profile| {\n            json!({\n                "id": profile.id,\n                "name": profile.name,\n                "manufacturer": profile.manufacturer,\n                "runtime": profile.runtime.id(),\n                "artifact": profile.artifact,\n                "nativeExecution": profile.native_execution,\n                "browserExecution": profile.browser_execution,\n                "description": profile.description,\n            })\n        })\n        .collect::<Vec<_>>();\n    let saturn_inputs = INPUT_TERMINALS\n        .iter()\n        .map(|terminal| {\n            json!({\n                "name": terminal.name,\n                "runtimeIndex": terminal.runtime_index,\n                "direction": "input",\n                "kind": format!("{:?}", terminal.kind).to_lowercase(),\n            })\n        })\n        .collect::<Vec<_>>();\n    let saturn_outputs = OUTPUT_TERMINALS\n        .iter()\n        .map(|terminal| {\n            json!({\n                "name": terminal.name,\n                "runtimeIndex": terminal.runtime_index,\n                "direction": "output",\n                "kind": format!("{:?}", terminal.kind).to_lowercase(),\n            })\n        })\n        .collect::<Vec<_>>();\n    let worlds = World::list()\n        .iter()\n        .map(|(id, description)| json!({ "id": id, "description": description }))\n        .collect::<Vec<_>>();\n\n    json!({\n        "boards": boards,\n        "socs": socs,\n        "controllers": controllers,\n        "pins": {\n            "phy6252": phy6252_pins,\n        },\n        "terminals": {\n            "saturn-plc": {\n                "inputs": saturn_inputs,\n                "outputs": saturn_outputs,\n            },\n        },\n        "worlds": worlds,\n    })'''
assert old_worlds in text, "registry body changed"
text = text.replace(old_worlds, new_worlds, 1)

path.write_text(text, encoding="utf-8")
