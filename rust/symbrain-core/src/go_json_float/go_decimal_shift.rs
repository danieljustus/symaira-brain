//! Go1.26.7 decimal.go bounded binary shifts with sticky truncation.
//! Copyright 2009 Go Authors; migration/licenses/go-strconv-bsd.txt.
use super::go_decimal::Decimal;

// Decimal digit growth and cutoff prefixes for multiplication by 2^k.
const LEFT: [(usize, &[u8]); 61] = [
    (0, b""),
    (1, b"5"),
    (1, b"25"),
    (1, b"125"),
    (2, b"625"),
    (2, b"3125"),
    (2, b"15625"),
    (3, b"78125"),
    (3, b"390625"),
    (3, b"1953125"),
    (4, b"9765625"),
    (4, b"48828125"),
    (4, b"244140625"),
    (4, b"1220703125"),
    (5, b"6103515625"),
    (5, b"30517578125"),
    (5, b"152587890625"),
    (6, b"762939453125"),
    (6, b"3814697265625"),
    (6, b"19073486328125"),
    (7, b"95367431640625"),
    (7, b"476837158203125"),
    (7, b"2384185791015625"),
    (7, b"11920928955078125"),
    (8, b"59604644775390625"),
    (8, b"298023223876953125"),
    (8, b"1490116119384765625"),
    (9, b"7450580596923828125"),
    (9, b"37252902984619140625"),
    (9, b"186264514923095703125"),
    (10, b"931322574615478515625"),
    (10, b"4656612873077392578125"),
    (10, b"23283064365386962890625"),
    (10, b"116415321826934814453125"),
    (11, b"582076609134674072265625"),
    (11, b"2910383045673370361328125"),
    (11, b"14551915228366851806640625"),
    (12, b"72759576141834259033203125"),
    (12, b"363797880709171295166015625"),
    (12, b"1818989403545856475830078125"),
    (13, b"9094947017729282379150390625"),
    (13, b"45474735088646411895751953125"),
    (13, b"227373675443232059478759765625"),
    (13, b"1136868377216160297393798828125"),
    (14, b"5684341886080801486968994140625"),
    (14, b"28421709430404007434844970703125"),
    (14, b"142108547152020037174224853515625"),
    (15, b"710542735760100185871124267578125"),
    (15, b"3552713678800500929355621337890625"),
    (15, b"17763568394002504646778106689453125"),
    (16, b"88817841970012523233890533447265625"),
    (16, b"444089209850062616169452667236328125"),
    (16, b"2220446049250313080847263336181640625"),
    (16, b"11102230246251565404236316680908203125"),
    (17, b"55511151231257827021181583404541015625"),
    (17, b"277555756156289135105907917022705078125"),
    (17, b"1387778780781445675529539585113525390625"),
    (18, b"6938893903907228377647697925567626953125"),
    (18, b"34694469519536141888238489627838134765625"),
    (18, b"173472347597680709441192448139190673828125"),
    (19, b"867361737988403547205962240695953369140625"),
];

impl Decimal {
    pub(super) fn shift(&mut self, mut amount: i64) {
        if self.length == 0 {
            return;
        }
        // Chunk by the SDK's native uint width, preserving intermediate
        // truncation/sticky state on both 32- and 64-bit targets.
        let maximum = i64::from(usize::BITS - 4);
        while amount > maximum {
            self.left(u32::try_from(maximum).unwrap());
            amount -= maximum;
        }
        while amount < -maximum {
            self.right(u32::try_from(maximum).unwrap());
            amount += maximum;
        }
        if amount > 0 {
            self.left(u32::try_from(amount).unwrap());
        }
        if amount < 0 {
            self.right(u32::try_from(-amount).unwrap());
        }
    }

    fn right(&mut self, shift: u32) {
        let (mut read, mut write, mut carry) = (0, 0, 0_u64);
        while carry >> shift == 0 {
            if read >= self.length {
                if carry == 0 {
                    self.length = 0;
                    return;
                }
                while carry >> shift == 0 {
                    carry *= 10;
                    read += 1;
                }
                break;
            }
            carry = carry * 10 + u64::from(self.digits[read] - b'0');
            read += 1;
        }
        self.point -= i64::try_from(read - 1).unwrap();
        let mask = (1_u64 << shift) - 1;
        while read < self.length {
            let digit = self.digits[read];
            let quotient = carry >> shift;
            carry &= mask;
            self.digits[write] = u8::try_from(quotient).unwrap() + b'0';
            write += 1;
            carry = carry * 10 + u64::from(digit - b'0');
            read += 1;
        }
        while carry > 0 {
            let quotient = carry >> shift;
            carry &= mask;
            if write < self.digits.len() {
                self.digits[write] = u8::try_from(quotient).unwrap() + b'0';
                write += 1;
            } else if quotient > 0 {
                self.truncated = true;
            }
            carry *= 10;
        }
        self.length = write;
        self.trim();
    }

    fn left(&mut self, shift: u32) {
        let (mut growth, cutoff) = LEFT[usize::try_from(shift).unwrap()];
        if &self.digits[..self.length] < cutoff {
            growth -= 1;
        }
        let mut write = self.length + growth;
        let mut carry = 0_u64;
        for read in (0..self.length).rev() {
            carry += u64::from(self.digits[read] - b'0') << shift;
            let remainder = carry % 10;
            carry /= 10;
            write -= 1;
            if write < self.digits.len() {
                self.digits[write] = u8::try_from(remainder).unwrap() + b'0';
            } else if remainder != 0 {
                self.truncated = true;
            }
        }
        while carry > 0 {
            let remainder = carry % 10;
            carry /= 10;
            write -= 1;
            if write < self.digits.len() {
                self.digits[write] = u8::try_from(remainder).unwrap() + b'0';
            } else if remainder != 0 {
                self.truncated = true;
            }
        }
        self.length = (self.length + growth).min(self.digits.len());
        self.point += i64::try_from(growth).unwrap();
        self.trim();
    }
}
