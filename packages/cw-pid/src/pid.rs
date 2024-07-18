use cosmwasm_std::SignedDecimal;

pub struct PID {
    p: u32,
    i: u32,
    d: u32,
}

impl PID {
    pub fn into_signed_decimal(&self) -> SignedPID {
        let p = SignedDecimal::bps(self.p as i64);
        let i = SignedDecimal::bps(self.i as i64);
        let d = SignedDecimal::bps(self.d as i64);

        SignedPID { p, i, d }
    }
}

pub struct SignedPID {
    p: SignedDecimal,
    i: SignedDecimal,
    d: SignedDecimal,
}

pub struct Input {
    pid: PID,
    input: SignedDecimal,
    target: SignedDecimal,
    dt: SignedDecimal,
    last_i: SignedDecimal,
    last_input: SignedDecimal,
}

pub struct Output {
    value: SignedDecimal,
    i: SignedDecimal,
}

pub fn pid(input: Input) -> Output {
    let pid = input.pid.into_signed_decimal();
    let error = input.target - input.input;

    let p = error * pid.p;
    let i = input.last_i + (error * pid.i * input.dt);
    let mut d = if input.last_input.is_zero() {
        SignedDecimal::zero()
    } else {
        input.input - input.last_input
    };

    d = d * pid.d / input.dt;

    Output { value: p + i - d, i }
}
