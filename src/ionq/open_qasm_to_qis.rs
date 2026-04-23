// This code is part of Qiskit.
//
// (C) Copyright IBM, IonQ 2025
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.

use serde_json::{json, Value};
use std::f64::consts::PI;

/// Translate an OpenQASM 3 program into IonQ QIS circuit JSON.
///
/// Returns a JSON string with `{"qubits": N, "circuit": [...]}` on success.
pub fn translate_qasm3_to_ionq_qis(src: &str) -> Result<String, String> {
    let mut t = Translator::new();
    t.parse(src)?;
    t.to_json()
}

struct Register {
    name: String,
    start: usize,
    size: usize,
}

struct Translator {
    total_qubits: usize,
    registers: Vec<Register>,
    circuit: Vec<Value>,
}

impl Translator {
    fn new() -> Self {
        Self {
            total_qubits: 0,
            registers: Vec::new(),
            circuit: Vec::new(),
        }
    }

    fn parse(&mut self, src: &str) -> Result<(), String> {
        let mut in_block_comment = false;

        for (idx, raw_line) in src.lines().enumerate() {
            let ln = idx + 1;
            let mut line = raw_line.trim();

            if in_block_comment {
                if let Some(pos) = line.find("*/") {
                    line = line[pos + 2..].trim();
                    in_block_comment = false;
                    if line.is_empty() {
                        continue;
                    }
                } else {
                    continue;
                }
            }

            if let Some(pos) = line.find("/*") {
                if let Some(end) = line[pos + 2..].find("*/") {
                    let joined =
                        format!("{} {}", &line[..pos], &line[pos + 2 + end + 2..]);
                    let joined = joined.trim();
                    if !joined.is_empty() {
                        self.process_line(ln, joined)?;
                    }
                    continue;
                } else {
                    line = line[..pos].trim();
                    in_block_comment = true;
                    if line.is_empty() {
                        continue;
                    }
                }
            }

            if let Some(pos) = line.find("//") {
                line = line[..pos].trim();
            }

            if line.is_empty() {
                continue;
            }

            self.process_line(ln, line)?;
        }

        Ok(())
    }

    fn process_line(&mut self, ln: usize, line: &str) -> Result<(), String> {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("openqasm")
            || lower.starts_with("include")
            || lower.starts_with("bit")
            || lower.starts_with("creg")
            || lower.starts_with("let ")
            || lower.starts_with("measure")
            || lower.starts_with("barrier")
            || lower.starts_with("reset")
        {
            return Ok(());
        }

        if lower.starts_with("qubit") {
            return self.parse_qubit_decl(ln, line);
        }
        if lower.starts_with("qreg") {
            return self.parse_qreg_decl(ln, line);
        }

        self.parse_gate(ln, line)
    }

    fn parse_qubit_decl(&mut self, ln: usize, line: &str) -> Result<(), String> {
        let line = line.trim_end_matches(';').trim();
        let rest = &line["qubit".len()..].trim_start();

        if rest.starts_with('[') {
            let bracket_end = rest
                .find(']')
                .ok_or_else(|| format!("line {ln}: missing ']' in qubit declaration"))?;
            let size: usize = rest[1..bracket_end]
                .trim()
                .parse()
                .map_err(|_| format!("line {ln}: invalid qubit size"))?;
            let name = rest[bracket_end + 1..].trim().to_string();
            let start = self.total_qubits;
            self.registers.push(Register { name, start, size });
            self.total_qubits += size;
        } else {
            let name = rest.to_string();
            let start = self.total_qubits;
            self.registers.push(Register {
                name,
                start,
                size: 1,
            });
            self.total_qubits += 1;
        }

        Ok(())
    }

    fn parse_qreg_decl(&mut self, ln: usize, line: &str) -> Result<(), String> {
        let line = line.trim_end_matches(';').trim();
        let rest = &line["qreg".len()..].trim_start();

        let bracket_start = rest
            .find('[')
            .ok_or_else(|| format!("line {ln}: missing '[' in qreg declaration"))?;
        let bracket_end = rest
            .find(']')
            .ok_or_else(|| format!("line {ln}: missing ']' in qreg declaration"))?;

        let name = rest[..bracket_start].trim().to_string();
        let size: usize = rest[bracket_start + 1..bracket_end]
            .trim()
            .parse()
            .map_err(|_| format!("line {ln}: invalid qreg size"))?;

        let start = self.total_qubits;
        self.registers.push(Register { name, start, size });
        self.total_qubits += size;

        Ok(())
    }

    fn resolve_qubit(&self, operand: &str) -> Result<usize, String> {
        let operand = operand.trim();
        if let Some(bracket_start) = operand.find('[') {
            let bracket_end = operand
                .find(']')
                .ok_or_else(|| format!("missing ']' in qubit reference '{operand}'"))?;
            let name = &operand[..bracket_start];
            let index: usize = operand[bracket_start + 1..bracket_end]
                .trim()
                .parse()
                .map_err(|_| format!("invalid qubit index in '{operand}'"))?;

            for reg in &self.registers {
                if reg.name == name {
                    if index >= reg.size {
                        return Err(format!(
                            "qubit index {index} out of range for register '{name}' (size {})",
                            reg.size
                        ));
                    }
                    return Ok(reg.start + index);
                }
            }
            Err(format!("unknown qubit register '{name}'"))
        } else {
            for reg in &self.registers {
                if reg.name == operand {
                    if reg.size != 1 {
                        return Err(format!(
                            "qubit register '{operand}' has size {}; use indexed access",
                            reg.size
                        ));
                    }
                    return Ok(reg.start);
                }
            }
            Err(format!("unknown qubit '{operand}'"))
        }
    }

    fn parse_gate(&mut self, ln: usize, line: &str) -> Result<(), String> {
        let line = line.trim_end_matches(';').trim();
        if line.is_empty() {
            return Ok(());
        }

        let (gate_name, params, operands_str) = if let Some(paren_start) = line.find('(') {
            let paren_end = line
                .find(')')
                .ok_or_else(|| format!("line {ln}: missing ')' in gate parameters"))?;
            let name = line[..paren_start].trim();
            let params: Vec<f64> = line[paren_start + 1..paren_end]
                .split(',')
                .map(|p| eval_expr(p.trim()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("line {ln}: {e}"))?;
            let operands = line[paren_end + 1..].trim();
            (name, params, operands)
        } else {
            let first_space = line
                .find(char::is_whitespace)
                .ok_or_else(|| format!("line {ln}: no operands for gate '{line}'"))?;
            let name = line[..first_space].trim();
            let operands = line[first_space..].trim();
            (name, Vec::new(), operands)
        };

        let operands: Vec<&str> = operands_str.split(',').map(|s| s.trim()).collect();
        let qubits: Vec<usize> = operands
            .iter()
            .map(|op| self.resolve_qubit(op))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("line {ln}: {e}"))?;

        self.emit_gate(ln, gate_name, &params, &qubits)
    }

    fn emit_gate(
        &mut self,
        ln: usize,
        name: &str,
        params: &[f64],
        qubits: &[usize],
    ) -> Result<(), String> {
        let lower = name.to_lowercase();
        match lower.as_str() {
            "id" | "i" => return Ok(()),

            "h" | "x" | "y" | "z" | "s" | "t" | "sx" => {
                expect_qubits(ln, name, qubits, 1)?;
                let ionq = match lower.as_str() {
                    "sx" => "v",
                    other => other,
                };
                self.circuit
                    .push(json!({"gate": ionq, "target": qubits[0]}));
            }
            "sdg" | "si" => {
                expect_qubits(ln, name, qubits, 1)?;
                self.circuit
                    .push(json!({"gate": "si", "target": qubits[0]}));
            }
            "tdg" | "ti" => {
                expect_qubits(ln, name, qubits, 1)?;
                self.circuit
                    .push(json!({"gate": "ti", "target": qubits[0]}));
            }
            "sxdg" | "vi" => {
                expect_qubits(ln, name, qubits, 1)?;
                self.circuit
                    .push(json!({"gate": "vi", "target": qubits[0]}));
            }
            "v" => {
                expect_qubits(ln, name, qubits, 1)?;
                self.circuit
                    .push(json!({"gate": "v", "target": qubits[0]}));
            }

            "rx" | "ry" | "rz" => {
                expect_qubits(ln, name, qubits, 1)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit.push(
                    json!({"gate": lower.as_str(), "rotation": turns, "target": qubits[0]}),
                );
            }

            "u1" | "p" => {
                expect_qubits(ln, name, qubits, 1)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit
                    .push(json!({"gate": "rz", "rotation": turns, "target": qubits[0]}));
            }

            "u3" | "u" => {
                expect_qubits(ln, name, qubits, 1)?;
                expect_params(ln, name, params, 3)?;
                let theta = params[0] / (2.0 * PI);
                let phi = params[1] / (2.0 * PI);
                let lambda = params[2] / (2.0 * PI);
                let q = qubits[0];
                self.circuit
                    .push(json!({"gate": "rz", "rotation": lambda, "target": q}));
                self.circuit
                    .push(json!({"gate": "ry", "rotation": theta, "target": q}));
                self.circuit
                    .push(json!({"gate": "rz", "rotation": phi, "target": q}));
            }

            "cx" | "cnot" => {
                expect_qubits(ln, name, qubits, 2)?;
                self.circuit.push(
                    json!({"gate": "cnot", "control": qubits[0], "target": qubits[1]}),
                );
            }
            "cz" => {
                expect_qubits(ln, name, qubits, 2)?;
                self.circuit.push(
                    json!({"gate": "cz", "control": qubits[0], "target": qubits[1]}),
                );
            }
            "swap" => {
                expect_qubits(ln, name, qubits, 2)?;
                self.circuit
                    .push(json!({"gate": "swap", "targets": [qubits[0], qubits[1]]}));
            }

            "rxx" | "xx" => {
                expect_qubits(ln, name, qubits, 2)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit.push(
                    json!({"gate": "xx", "rotation": turns, "targets": [qubits[0], qubits[1]]}),
                );
            }
            "ryy" | "yy" => {
                expect_qubits(ln, name, qubits, 2)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit.push(
                    json!({"gate": "yy", "rotation": turns, "targets": [qubits[0], qubits[1]]}),
                );
            }
            "rzz" | "zz" => {
                expect_qubits(ln, name, qubits, 2)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit.push(
                    json!({"gate": "zz", "rotation": turns, "targets": [qubits[0], qubits[1]]}),
                );
            }

            "ccx" | "ccnot" | "toffoli" => {
                expect_qubits(ln, name, qubits, 3)?;
                self.circuit.push(
                    json!({"gate": "x", "controls": [qubits[0], qubits[1]], "target": qubits[2]}),
                );
            }
            "cswap" | "fredkin" => {
                expect_qubits(ln, name, qubits, 3)?;
                self.circuit.push(
                    json!({"gate": "swap", "control": qubits[0], "targets": [qubits[1], qubits[2]]}),
                );
            }

            "gpi" => {
                expect_qubits(ln, name, qubits, 1)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit
                    .push(json!({"gate": "gpi", "phase": turns, "target": qubits[0]}));
            }
            "gpi2" => {
                expect_qubits(ln, name, qubits, 1)?;
                expect_params(ln, name, params, 1)?;
                let turns = params[0] / (2.0 * PI);
                self.circuit
                    .push(json!({"gate": "gpi2", "phase": turns, "target": qubits[0]}));
            }
            "ms" => {
                expect_qubits(ln, name, qubits, 2)?;
                match params.len() {
                    0 => {
                        self.circuit.push(
                            json!({"gate": "ms", "targets": [qubits[0], qubits[1]]}),
                        );
                    }
                    3 => {
                        let t: Vec<f64> = params.iter().map(|p| p / (2.0 * PI)).collect();
                        self.circuit.push(json!({
                            "gate": "ms",
                            "phases": [t[0], t[1]],
                            "angle": t[2],
                            "targets": [qubits[0], qubits[1]]
                        }));
                    }
                    n => {
                        return Err(format!(
                            "line {ln}: gate 'ms' expects 0 or 3 parameters, got {n}"
                        ))
                    }
                }
            }

            _ => return Err(format!("line {ln}: unsupported gate '{name}'")),
        }

        Ok(())
    }

    fn to_json(&self) -> Result<String, String> {
        if self.total_qubits == 0 {
            return Err("no qubit declarations found".to_string());
        }
        let result = json!({
            "qubits": self.total_qubits,
            "circuit": self.circuit,
        });
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}

fn expect_qubits(ln: usize, name: &str, qubits: &[usize], expected: usize) -> Result<(), String> {
    if qubits.len() != expected {
        return Err(format!(
            "line {ln}: gate '{name}' expects {expected} qubit(s), got {}",
            qubits.len()
        ));
    }
    Ok(())
}

fn expect_params(ln: usize, name: &str, params: &[f64], expected: usize) -> Result<(), String> {
    if params.len() != expected {
        return Err(format!(
            "line {ln}: gate '{name}' expects {expected} parameter(s), got {}",
            params.len()
        ));
    }
    Ok(())
}

fn eval_expr(expr: &str) -> Result<f64, String> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err("empty expression".to_string());
    }

    // Handle leading unary minus: only if the rest is not another operator expression
    if expr.starts_with('-') && !expr[1..].contains(['+', '-', '*', '/']) {
        return eval_atom(&expr[1..]).map(|v| -v);
    }

    // Addition / subtraction (lowest precedence, scan right-to-left)
    if let Some(pos) = find_op(expr, &['+', '-']) {
        let left = eval_expr(&expr[..pos])?;
        let right = eval_expr(&expr[pos + 1..])?;
        return Ok(if expr.as_bytes()[pos] == b'+' {
            left + right
        } else {
            left - right
        });
    }

    // Multiplication / division
    if let Some(pos) = find_op(expr, &['*', '/']) {
        let left = eval_expr(&expr[..pos])?;
        let right = eval_expr(&expr[pos + 1..])?;
        return Ok(if expr.as_bytes()[pos] == b'*' {
            left * right
        } else {
            left / right
        });
    }

    // Parenthesised sub-expression
    if expr.starts_with('(') && expr.ends_with(')') {
        return eval_expr(&expr[1..expr.len() - 1]);
    }

    eval_atom(expr)
}

fn eval_atom(s: &str) -> Result<f64, String> {
    let s = s.trim();
    match s {
        "pi" => Ok(PI),
        "tau" => Ok(2.0 * PI),
        _ => s
            .parse::<f64>()
            .map_err(|_| format!("cannot evaluate expression '{s}'")),
    }
}

fn find_op(expr: &str, ops: &[char]) -> Option<usize> {
    let bytes = expr.as_bytes();
    let mut depth = 0i32;
    for i in (1..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            c if depth == 0 && ops.contains(&(c as char)) => {
                let prev = bytes[i - 1];
                if prev != b'(' && prev != b'+' && prev != b'-' && prev != b'*' && prev != b'/' {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parse_result(src: &str) -> Value {
        let json_str = translate_qasm3_to_ionq_qis(src).unwrap();
        serde_json::from_str(&json_str).unwrap()
    }

    #[test]
    fn bell_state() {
        let qasm = "\
OPENQASM 3.0;
include \"stdgates.inc\";
qubit[2] q;
h q[0];
cx q[0], q[1];
";
        let result = parse_result(qasm);
        assert_eq!(result["qubits"], 2);
        let circuit = result["circuit"].as_array().unwrap();
        assert_eq!(circuit.len(), 2);
        assert_eq!(circuit[0], json!({"gate": "h", "target": 0}));
        assert_eq!(
            circuit[1],
            json!({"gate": "cnot", "control": 0, "target": 1})
        );
    }

    #[test]
    fn parametric_gates() {
        let qasm = "\
OPENQASM 3.0;
qubit[1] q;
rx(pi) q[0];
ry(pi/2) q[0];
rz(pi/4) q[0];
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        assert_eq!(circuit.len(), 3);

        assert_eq!(circuit[0]["gate"], "rx");
        assert!((circuit[0]["rotation"].as_f64().unwrap() - 0.5).abs() < 1e-10);

        assert_eq!(circuit[1]["gate"], "ry");
        assert!((circuit[1]["rotation"].as_f64().unwrap() - 0.25).abs() < 1e-10);

        assert_eq!(circuit[2]["gate"], "rz");
        assert!((circuit[2]["rotation"].as_f64().unwrap() - 0.125).abs() < 1e-10);
    }

    #[test]
    fn multiple_registers() {
        let qasm = "\
OPENQASM 3.0;
qubit[2] a;
qubit[3] b;
cx a[1], b[0];
";
        let result = parse_result(qasm);
        assert_eq!(result["qubits"], 5);
        let circuit = result["circuit"].as_array().unwrap();
        // a[1] = index 1, b[0] = index 2
        assert_eq!(
            circuit[0],
            json!({"gate": "cnot", "control": 1, "target": 2})
        );
    }

    #[test]
    fn single_qubit_register() {
        let qasm = "\
OPENQASM 3.0;
qubit q;
h q;
";
        let result = parse_result(qasm);
        assert_eq!(result["qubits"], 1);
        assert_eq!(
            result["circuit"].as_array().unwrap()[0],
            json!({"gate": "h", "target": 0})
        );
    }

    #[test]
    fn qreg_syntax() {
        let qasm = "\
OPENQASM 3.0;
qreg q[2];
h q[0];
";
        let result = parse_result(qasm);
        assert_eq!(result["qubits"], 2);
    }

    #[test]
    fn comments_and_blank_lines() {
        let qasm = "\
OPENQASM 3.0;
// This is a comment
qubit[2] q;

/* block comment */
h q[0]; // inline comment

cx q[0], q[1];
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        assert_eq!(circuit.len(), 2);
    }

    #[test]
    fn u3_decomposition() {
        let qasm = "\
OPENQASM 3.0;
qubit[1] q;
u3(pi/2, 0, pi) q[0];
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        // u3(theta, phi, lambda) -> rz(lambda), ry(theta), rz(phi)
        assert_eq!(circuit.len(), 3);
        assert_eq!(circuit[0]["gate"], "rz"); // lambda = pi -> 0.5 turns
        assert!((circuit[0]["rotation"].as_f64().unwrap() - 0.5).abs() < 1e-10);
        assert_eq!(circuit[1]["gate"], "ry"); // theta = pi/2 -> 0.25 turns
        assert!((circuit[1]["rotation"].as_f64().unwrap() - 0.25).abs() < 1e-10);
        assert_eq!(circuit[2]["gate"], "rz"); // phi = 0 -> 0 turns
        assert!((circuit[2]["rotation"].as_f64().unwrap()).abs() < 1e-10);
    }

    #[test]
    fn swap_gate() {
        let qasm = "\
OPENQASM 3.0;
qubit[2] q;
swap q[0], q[1];
";
        let result = parse_result(qasm);
        assert_eq!(
            result["circuit"].as_array().unwrap()[0],
            json!({"gate": "swap", "targets": [0, 1]})
        );
    }

    #[test]
    fn toffoli_gate() {
        let qasm = "\
OPENQASM 3.0;
qubit[3] q;
ccx q[0], q[1], q[2];
";
        let result = parse_result(qasm);
        assert_eq!(
            result["circuit"].as_array().unwrap()[0],
            json!({"gate": "x", "controls": [0, 1], "target": 2})
        );
    }

    #[test]
    fn measure_and_barrier_are_skipped() {
        let qasm = "\
OPENQASM 3.0;
qubit[2] q;
bit[2] c;
h q[0];
barrier q[0], q[1];
measure q -> c;
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        assert_eq!(circuit.len(), 1);
        assert_eq!(circuit[0]["gate"], "h");
    }

    #[test]
    fn identity_gates_are_skipped() {
        let qasm = "\
OPENQASM 3.0;
qubit[1] q;
id q[0];
h q[0];
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        assert_eq!(circuit.len(), 1);
        assert_eq!(circuit[0]["gate"], "h");
    }

    #[test]
    fn negative_rotation() {
        let qasm = "\
OPENQASM 3.0;
qubit[1] q;
rz(-pi/4) q[0];
";
        let result = parse_result(qasm);
        let circuit = result["circuit"].as_array().unwrap();
        assert!((circuit[0]["rotation"].as_f64().unwrap() + 0.125).abs() < 1e-10);
    }

    #[test]
    fn unknown_gate_is_error() {
        let qasm = "\
OPENQASM 3.0;
qubit[1] q;
foobar q[0];
";
        let err = translate_qasm3_to_ionq_qis(qasm).unwrap_err();
        assert!(err.contains("unsupported gate"));
    }

    #[test]
    fn no_qubits_is_error() {
        let qasm = "OPENQASM 3.0;\n";
        let err = translate_qasm3_to_ionq_qis(qasm).unwrap_err();
        assert!(err.contains("no qubit declarations"));
    }

    #[test]
    fn qubit_out_of_range_is_error() {
        let qasm = "\
OPENQASM 3.0;
qubit[2] q;
h q[5];
";
        let err = translate_qasm3_to_ionq_qis(qasm).unwrap_err();
        assert!(err.contains("out of range"));
    }

    #[test]
    fn expr_arithmetic() {
        assert!((eval_expr("pi").unwrap() - PI).abs() < 1e-10);
        assert!((eval_expr("pi/2").unwrap() - PI / 2.0).abs() < 1e-10);
        assert!((eval_expr("2*pi").unwrap() - 2.0 * PI).abs() < 1e-10);
        assert!((eval_expr("-pi").unwrap() + PI).abs() < 1e-10);
        assert!((eval_expr("pi/2+pi/4").unwrap() - 3.0 * PI / 4.0).abs() < 1e-10);
        assert!((eval_expr("(pi)").unwrap() - PI).abs() < 1e-10);
    }
}
