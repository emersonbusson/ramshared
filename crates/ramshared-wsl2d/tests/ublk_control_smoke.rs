use ramshared_wsl2d::{ublk, ublk_control};

#[test]
#[ignore = "requires root and /dev/ublk-control; does not create a ublk device"]
fn get_features_from_ublk_control_without_creating_device() {
    let report = ublk_control::get_features("/dev/ublk-control").expect("ublk GET_FEATURES");

    assert_ne!(report.features & ublk::UBLK_F_CMD_IOCTL_ENCODE, 0);
    assert_eq!(report.features & ublk::UBLK_F_SUPPORT_ZERO_COPY, 0);
}
