//! PTP (ISO 15740) and Fujifilm vendor codes.
//! Fuji codes come from public reverse engineering (rawji, fudge, FilmKit).

pub const FUJI_VID: u16 = 0x04CB;

pub mod op {
    pub const GET_DEVICE_INFO: u16 = 0x1001;
    pub const OPEN_SESSION: u16 = 0x1002;
    pub const CLOSE_SESSION: u16 = 0x1003;
    pub const GET_STORAGE_IDS: u16 = 0x1004;
    pub const GET_STORAGE_INFO: u16 = 0x1005;
    pub const GET_NUM_OBJECTS: u16 = 0x1006;
    pub const GET_OBJECT_HANDLES: u16 = 0x1007;
    pub const GET_OBJECT_INFO: u16 = 0x1008;
    pub const GET_OBJECT: u16 = 0x1009;
    pub const GET_THUMB: u16 = 0x100A;
    pub const DELETE_OBJECT: u16 = 0x100B;
    pub const SEND_OBJECT_INFO: u16 = 0x100C;
    pub const SEND_OBJECT: u16 = 0x100D;
    pub const GET_DEVICE_PROP_DESC: u16 = 0x1014;
    pub const GET_DEVICE_PROP_VALUE: u16 = 0x1015;
    pub const SET_DEVICE_PROP_VALUE: u16 = 0x1016;
    /// Fuji vendor SendObjectInfo (used by X RAW Studio to upload RAF)
    pub const FUJI_SEND_OBJECT_INFO: u16 = 0x900C;
    /// Fuji vendor SendObject2
    pub const FUJI_SEND_OBJECT: u16 = 0x900D;
}

pub mod prop {
    pub const START_RAW_CONVERSION: u16 = 0xD183;
    pub const RAW_CONV_PROFILE: u16 = 0xD185;
}

pub mod format {
    pub const ASSOCIATION: u16 = 0x3001;
    pub const EXIF_JPEG: u16 = 0x3801;
    /// Object format X RAW Studio uses when uploading a RAF.
    pub const FUJI_RAF: u16 = 0xF802;
    /// Advertised in ImageFormats only in USB RAW CONV mode (X-Pro3). Card-reader mode
    /// reports RAFs on the card as 0x3800 (undefined) instead.
    pub const FUJI_RAF_RAWCONV: u16 = 0xB103;
}

pub mod resp {
    pub const OK: u16 = 0x2001;
    pub const GENERAL_ERROR: u16 = 0x2002;
    pub const SESSION_NOT_OPEN: u16 = 0x2003;
    pub const OPERATION_NOT_SUPPORTED: u16 = 0x2005;
    pub const DEVICE_BUSY: u16 = 0x2019;
    pub const SESSION_ALREADY_OPEN: u16 = 0x201E;
}

pub fn resp_name(code: u16) -> String {
    let n = match code {
        0x2001 => "OK",
        0x2002 => "GeneralError",
        0x2003 => "SessionNotOpen",
        0x2004 => "InvalidTransactionID",
        0x2005 => "OperationNotSupported",
        0x2006 => "ParameterNotSupported",
        0x2007 => "IncompleteTransfer",
        0x2008 => "InvalidStorageID",
        0x2009 => "InvalidObjectHandle",
        0x200A => "DevicePropNotSupported",
        0x200B => "InvalidObjectFormatCode",
        0x200C => "StoreFull",
        0x200F => "AccessDenied",
        0x2010 => "NoThumbnailPresent",
        0x2019 => "DeviceBusy",
        0x201B => "InvalidDevicePropFormat",
        0x201C => "InvalidDevicePropValue",
        0x201D => "InvalidParameter",
        0x201E => "SessionAlreadyOpen",
        0x201F => "TransactionCancelled",
        _ => return format!("0x{code:04X}"),
    };
    format!("{n} (0x{code:04X})")
}

pub fn op_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x1001 => "GetDeviceInfo",
        0x1002 => "OpenSession",
        0x1003 => "CloseSession",
        0x1004 => "GetStorageIDs",
        0x1005 => "GetStorageInfo",
        0x1006 => "GetNumObjects",
        0x1007 => "GetObjectHandles",
        0x1008 => "GetObjectInfo",
        0x1009 => "GetObject",
        0x100A => "GetThumb",
        0x100B => "DeleteObject",
        0x100C => "SendObjectInfo",
        0x100D => "SendObject",
        0x1014 => "GetDevicePropDesc",
        0x1015 => "GetDevicePropValue",
        0x1016 => "SetDevicePropValue",
        0x101B => "GetPartialObject",
        0x900C => "Fuji SendObjectInfo",
        0x900D => "Fuji SendObject2",
        _ => return None,
    })
}

pub fn prop_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0xD001 => "FilmSimulation",
        0xD003 => "DRangeMode",
        0xD007 => "ColorTemperature",
        0xD00A => "NoiseReduction",
        0xD017 => "GrainEffect",
        0xD171 => "RawConversionEdit",
        0xD183 => "StartRawConversion",
        0xD184 => "IOPCodes",
        0xD185 => "RawConvProfile",
        0xD186 => "FirmwareVersion",
        0xD187 => "FirmwareVersion2",
        0xD18C => "PresetSlot",
        0xD18D => "PresetName",
        _ => return None,
    })
}
