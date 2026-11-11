use super::read_paths_from_data_object;
use windows::Win32::System::Com::IDataObject;
use windows::core::Ref;

#[test]
fn null_data_object_returns_error() {
    let null: Ref<'_, IDataObject> = Ref::default();
    assert!(read_paths_from_data_object(null).is_err());
}
