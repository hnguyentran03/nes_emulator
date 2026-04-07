pub const NES_TAG: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A];
pub const PRG_ROM_PAGE_SIZE: usize = 0x4000;
pub const CHR_ROM_PAGE_SIZE: usize = 0x2000;
pub const HEADER_SIZE: usize = 0x10;
pub const TRAINER_SIZE: usize = 0x200;

pub const PRG_ROM: u16 = 0x8000;
pub const PRG_ROM_END: u16 = 0xFFFF;

#[derive(Debug, PartialEq)]
pub enum Mirror {
    Horizontal,
    Vertical,
    FourScreen,
}

pub struct Rom {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: u8,
    pub screen_mirror: Mirror,
}

impl Rom {
    pub fn new(data: &[u8]) -> Result<Self, String> {
        if data[0..4] != NES_TAG {
            // First 4 bytes are used for the NES tag
            return Err("Invalid NES file".to_string());
        }

        // Contorl bytes have additional information about the ROM
        let control_byte_1 = data[6];
        let control_byte_2 = data[7];

        // Construct mapper on the control bytes (upper 4 bits of the 2nd byte and lower 4 bits of the 1st byte)
        let mapper = (control_byte_2 & 0b1111_0000) | (control_byte_1 >> 4);

        // Least 2 significant bits of the 2nd control byte determines the ines version
        let ines_version = control_byte_2 >> 2 & 0b11;
        if ines_version != 0 {
            // Both bits must be 0 for NES 1.0
            return Err("Unsupported NES file version".to_string());
        }

        let is_four_screen = control_byte_1 & 0b1000 != 0;
        let is_vertical_mirror = control_byte_1 & 0b1 != 0;
        let screen_mirror = match (is_four_screen, is_vertical_mirror) {
            (false, false) => Mirror::Horizontal,
            (false, true) => Mirror::Vertical,
            (true, _) => Mirror::FourScreen,
        };

        let skip_trainer = control_byte_1 & 0b100 != 0;

        let prg_rom_size = data[4] as usize * PRG_ROM_PAGE_SIZE;
        let chr_rom_size = data[5] as usize * CHR_ROM_PAGE_SIZE;

        let prg_rom_start = HEADER_SIZE + if skip_trainer { TRAINER_SIZE } else { 0 };
        let chr_rom_start = prg_rom_start + prg_rom_size;

        let prg_rom = data[prg_rom_start..prg_rom_start + prg_rom_size].to_vec();
        let chr_rom = data[chr_rom_start..chr_rom_start + chr_rom_size].to_vec();

        Ok(Rom {
            prg_rom,
            chr_rom,
            mapper,
            screen_mirror,
        })
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    struct ROMFile {
        header: Vec<u8>,
        trainer: Option<Vec<u8>>,
        pgp_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    }

    fn create_rom(file: ROMFile) -> Vec<u8> {
        let mut result = Vec::with_capacity(
            file.header.len()
                + file.trainer.as_ref().map_or(0, |t| t.len())
                + file.pgp_rom.len()
                + file.chr_rom.len(),
        );

        result.extend(&file.header);
        if let Some(t) = file.trainer {
            result.extend(t);
        }
        result.extend(&file.pgp_rom);
        result.extend(&file.chr_rom);

        result
    }

    pub fn build_test_rom(program: Vec<u8>) -> Rom {
        let mut pgp_rom_contents = program;
        pgp_rom_contents.resize(2 * PRG_ROM_PAGE_SIZE, 0);

        let file = ROMFile {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x02, 0x01, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: None,
            pgp_rom: pgp_rom_contents,
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        };

        let rom_data = create_rom(file);

        Rom::new(&rom_data).unwrap()
    }

    #[test]
    fn rejects_invalid_nes_tag() {
        // The first 4 bytes are used for the NES tag so if they are not present, the file is invalid
        let file = ROMFile {
            header: vec![
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: None,
            pgp_rom: vec![1; 2 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        };
        let rom_data = create_rom(file);

        let result = Rom::new(&rom_data);

        assert!(matches!(result, Err(msg) if msg == "Invalid NES file"));
    }

    #[test]
    fn rejects_unsupported_ines_version() {
        // Byte 7 bits 2–3 != 0 marks non–NES 1.0
        let file = ROMFile {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: None,
            pgp_rom: vec![],
            chr_rom: vec![],
        };
        let rom_data = create_rom(file);

        let result = Rom::new(&rom_data);

        assert!(matches!(
            result,
            Err(msg) if msg == "Unsupported NES file version"
        ));
    }

    #[test]
    fn parses_mapper_and_rom_data_without_trainer() {
        // Set the mapper to 0x23 and set the screen mirror to vertical mirroring
        let mut pgp_rom = vec![0u8; PRG_ROM_PAGE_SIZE];
        pgp_rom[0] = 0x00;
        pgp_rom[1] = 0x01;
        let mut chr_rom = vec![0u8; CHR_ROM_PAGE_SIZE];
        chr_rom[0] = 0x80;

        let file = ROMFile {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x01, 0x01, 0x31, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: None,
            pgp_rom,
            chr_rom,
        };
        let rom_data = create_rom(file);

        let rom = Rom::new(&rom_data).unwrap();

        assert_eq!(rom.mapper, 0x23);
        assert_eq!(rom.screen_mirror, Mirror::Vertical);
        assert_eq!(rom.prg_rom.len(), PRG_ROM_PAGE_SIZE);
        assert_eq!(rom.chr_rom.len(), CHR_ROM_PAGE_SIZE);
        assert_eq!(rom.prg_rom[0], 0x00);
        assert_eq!(rom.prg_rom[1], 0x01);
        assert_eq!(rom.chr_rom[0], 0x80);
    }

    #[test]
    fn skips_trainer_when_present() {
        // Set the mapper to 0x04 and skip the trainer
        let mut pgp_rom = vec![0u8; PRG_ROM_PAGE_SIZE];
        pgp_rom[0] = 0x00;
        let mut chr_rom = vec![0u8; CHR_ROM_PAGE_SIZE];
        chr_rom[0] = 0x80;

        let file = ROMFile {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x01, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: Some(vec![0xFF; TRAINER_SIZE]),
            pgp_rom,
            chr_rom,
        };
        let rom_data = create_rom(file);

        let rom = Rom::new(&rom_data).unwrap();

        assert_eq!(rom.prg_rom[0], 0x00);
        assert_eq!(rom.chr_rom[0], 0x80);
    }

    #[test]
    fn uses_four_screen_mirroring_when_flag_set() {
        // Sets the mapper to 0x09 and sets the screen mirror to four-screen mirroring
        let file = ROMFile {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x01, 0x01, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ],
            trainer: None,
            pgp_rom: vec![1; 1 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        };
        let rom_data = create_rom(file);

        let rom = Rom::new(&rom_data).unwrap();

        assert_eq!(rom.screen_mirror, Mirror::FourScreen);
    }
}
