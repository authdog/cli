//! Agent Sign-In headless proof: P-256 PKCS#8 PEM → ES256 (IEEE P1363) signature.

const std = @import("std");

const Ecdsa = std.crypto.sign.ecdsa.EcdsaP256Sha256;

pub const SignError = error{
    InvalidPem,
    InvalidDer,
    InvalidKeyLength,
    NotEcPrivateKey,
    IdentityElement,
};

/// Sign `message` with a PKCS#8 or SEC1 EC private key PEM (P-256).
pub fn signAuthToken(message: []const u8, pem: []const u8) SignError![Ecdsa.Signature.encoded_length]u8 {
    const scalar = try loadP256ScalarFromPem(pem);
    const sk = Ecdsa.SecretKey.fromBytes(scalar) catch return error.InvalidKeyLength;
    const kp = Ecdsa.KeyPair.fromSecretKey(sk) catch return error.IdentityElement;
    const sig = kp.sign(message, null) catch return error.IdentityElement;
    return sig.toBytes();
}

/// Base64url-encode `bytes` (no padding).
pub fn base64UrlEncode(allocator: std.mem.Allocator, bytes: []const u8) ![]u8 {
    const enc = std.base64.url_safe_no_pad.Encoder;
    const len = enc.calcSize(bytes.len);
    const out = try allocator.alloc(u8, len);
    _ = enc.encode(out, bytes);
    return out;
}

fn loadP256ScalarFromPem(pem: []const u8) SignError![32]u8 {
    const der = decodePemBlock(pem) orelse return error.InvalidPem;
    if (parseEcPrivateKeyScalar(der)) |scalar| return scalar;
    if (parsePkcs8EcScalar(der)) |scalar| return scalar;
    return error.NotEcPrivateKey;
}

fn decodePemBlock(pem: []const u8) ?[]const u8 {
    var b64_body: std.ArrayList(u8) = .empty;
    defer b64_body.deinit(std.heap.page_allocator);
    var it = std.mem.splitScalar(u8, pem, '\n');
    var in_body = false;
    while (it.next()) |line| {
        const trimmed = std.mem.trim(u8, line, " \r\t");
        if (std.mem.startsWith(u8, trimmed, "-----BEGIN")) {
            in_body = true;
            continue;
        }
        if (std.mem.startsWith(u8, trimmed, "-----END")) break;
        if (in_body and trimmed.len != 0) {
            b64_body.appendSlice(std.heap.page_allocator, trimmed) catch return null;
        }
    }
    if (b64_body.items.len == 0) return null;
    const decoder = std.base64.standard.Decoder;
    const der_len = decoder.calcSizeForSlice(b64_body.items) catch return null;
    var der_buf: [512]u8 = undefined;
    if (der_len > der_buf.len) return null;
    decoder.decode(der_buf[0..der_len], b64_body.items) catch return null;
    return der_buf[0..der_len];
}

fn parsePkcs8EcScalar(der: []const u8) ?[32]u8 {
    var reader: std.Io.Reader = .fixed(der);
    if ((reader.takeByte() catch return null) != 0x30) return null;
    _ = readDerLength(&reader) catch return null;
    _ = readDerInteger(&reader) catch return null;
    _ = readDerSequence(&reader) catch return null;
    const inner = readDerOctetString(&reader) catch return null;
    return parseEcPrivateKeyScalar(inner);
}

fn parseEcPrivateKeyScalar(der: []const u8) ?[32]u8 {
    var reader: std.Io.Reader = .fixed(der);
    if ((reader.takeByte() catch return null) != 0x30) return null;
    _ = readDerLength(&reader) catch return null;
    const version = readDerInteger(&reader) catch return null;
    if (version.len != 1 or version[0] != 1) return null;
    const key_octet = readDerOctetString(&reader) catch return null;
    if (key_octet.len != 32) return null;
    var out: [32]u8 = undefined;
    @memcpy(&out, key_octet);
    return out;
}

fn readDerLength(reader: *std.Io.Reader) !usize {
    const first = try reader.takeByte();
    if (first & 0x80 == 0) return first;
    const count = first & 0x7f;
    if (count == 0 or count > 4) return error.InvalidDer;
    var len: usize = 0;
    var i: usize = 0;
    while (i < count) : (i += 1) {
        len = (len << 8) | (reader.takeByte() catch return error.InvalidDer);
    }
    return len;
}

fn readDerBytes(reader: *std.Io.Reader, len: usize) ![]const u8 {
    if (reader.seek + len > reader.buffer.len) return error.InvalidDer;
    const slice = reader.buffer[reader.seek .. reader.seek + len];
    reader.seek += len;
    return slice;
}

fn readDerInteger(reader: *std.Io.Reader) ![]const u8 {
    if ((reader.takeByte() catch return error.InvalidDer) != 0x02) return error.InvalidDer;
    const len = try readDerLength(reader);
    return readDerBytes(reader, len);
}

fn readDerOctetString(reader: *std.Io.Reader) ![]const u8 {
    if ((reader.takeByte() catch return error.InvalidDer) != 0x04) return error.InvalidDer;
    const len = try readDerLength(reader);
    return readDerBytes(reader, len);
}

fn readDerSequence(reader: *std.Io.Reader) ![]const u8 {
    if ((reader.takeByte() catch return error.InvalidDer) != 0x30) return error.InvalidDer;
    const len = try readDerLength(reader);
    return readDerBytes(reader, len);
}

test "signAuthToken matches node ieee p1363 golden vector" {
    const pem =
        \\-----BEGIN PRIVATE KEY-----
        \\MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgj71c1DtfdJGNPYS6
        \\oimAqFO7Uf9IbXsI1OM3fvUj2IKhRANCAARXHhAX/759VEGJ0GiDzwuF6J0K1NiR
        \\U/UVv7jx3nzJCD2LzKYU0p8vg12gvAlE1x/gx8A2A+VOa7WFp6s5SuXk
        \\-----END PRIVATE KEY-----
    ;
    const msg = "test-auth-token-12345";
    const expected_b64 = "OZtnEh-PsIHWJuJYNTz2hwfCKDuZquFUD7XS635u0vmWb5e2bKU_ZlDBZ9Yw1h4of14jggYug29bzfnHSE510g";

    const raw = try signAuthToken(msg, pem);
    const encoded = try base64UrlEncode(std.testing.allocator, &raw);
    defer std.testing.allocator.free(encoded);
    try std.testing.expectEqualStrings(expected_b64, encoded);
}
