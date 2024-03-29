pub trait BusInterface {
    fn read(&mut self, address: u16) -> u8;

    fn write(&mut self, address: u16, value: u8);

    fn nmi(&self) -> bool;

    fn acknowledge_nmi(&mut self);

    fn irq(&self) -> bool;
}
