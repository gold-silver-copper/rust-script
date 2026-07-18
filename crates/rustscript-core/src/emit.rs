use crate::CheckedProgram;

pub(crate) fn format(program: &CheckedProgram) -> String {
    let mut output = String::new();
    for function in &program.functions {
        output.push_str("fn ");
        output.push_str(&function.name);
        output.push('(');
        for (index, parameter) in function.parameters.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            output.push_str(&parameter.name);
            output.push_str(": ");
            output.push_str(type_name(parameter.ty));
        }
        output.push(')');
        if function.explicit_return {
            output.push_str(" -> ");
            output.push_str(type_name(function.return_type));
        }
        output.push_str(" {}\n");
    }
    output
}

fn type_name(ty: crate::Type) -> &'static str {
    match ty {
        crate::Type::I64 => "i64",
        crate::Type::Bool => "bool",
        crate::Type::Unit => "()",
    }
}
