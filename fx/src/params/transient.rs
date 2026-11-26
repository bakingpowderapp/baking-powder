use super::*;

params!(Transient {
    lead attack_db = 0.0, "attackDb", -Transient::ATTACK_DB..=Transient::ATTACK_DB;
    lead sustain_db = 0.0, "sustainDb", -Transient::SUSTAIN_DB..=Transient::SUSTAIN_DB;
    number speed = 1.0, "speed", Transient::MIN_SPEED..=Transient::MAX_SPEED;
    flag clip = false, "clip";
    number output_db = 0.0, "outputDb", -Transient::OUTPUT_DB..=Transient::OUTPUT_DB;
});

impl Transient {
    pub const ATTACK_DB: f64 = 15.0;
    pub const SUSTAIN_DB: f64 = 24.0;
    pub const OUTPUT_DB: f64 = 24.0;
    pub const MIN_SPEED: f64 = 0.2;
    pub const MAX_SPEED: f64 = 5.0;
}

table! {
    pub PARAMS: Transient {
        ATTACK = "SHAPE", "ATTACK", field!(attack_db), linear(-Transient::ATTACK_DB, Transient::ATTACK_DB);
        SUSTAIN = "SHAPE", "SUSTAIN", field!(sustain_db), linear(-Transient::SUSTAIN_DB, Transient::SUSTAIN_DB);
        SPEED = "TIME", "SPEED", field!(speed), log(Transient::MIN_SPEED, Transient::MAX_SPEED);
        LEVEL = "OUTPUT", "LEVEL", field!(output_db), linear(-Transient::OUTPUT_DB, Transient::OUTPUT_DB);
    }
}
