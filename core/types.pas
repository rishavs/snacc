

interface

type
    PxTypes = (
        PxType_Nothing,
        PxType_Byte,
        PxType_Char,
        PxType_String,
        PxType_Int,
        PxType_Int32,
        PxType_Int64,
        PxType_Float,
        PxType_Float32,
        PxType_Float64,
        PxType_Bool,
    );
    PxNothing = record
        Kind : [PxTypes.PxType_Nothing];
        value : string;
    end;
    PxType_Bool = record
        Kind := PxTypes.PxType_Bool;
        value := nil;
    end;

implementation
    
end.

