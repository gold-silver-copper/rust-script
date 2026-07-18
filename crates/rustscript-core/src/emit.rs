use crate::CheckedProgram;

pub(crate) fn format(program: &CheckedProgram) -> String {
    program.source.clone()
}
