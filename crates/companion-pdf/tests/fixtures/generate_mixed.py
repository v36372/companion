"""Generate mixed.pdf without external packages: text page, then image-only page."""
from pathlib import Path


def stream(data, extra=b""):
    return b"<< /Length %d %s >>\nstream\n" % (len(data), extra) + data + b"\nendstream"


objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    stream(b"BT /F1 24 Tf 60 700 Td (Companion text page) Tj ET"),
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Scan 8 0 R >> >> /Contents 7 0 R >>",
    stream(b"q 400 0 0 500 100 150 cm /Scan Do Q"),
    stream(bytes([210, 220, 230, 160, 180, 200, 160, 180, 200, 210, 220, 230]),
           b"/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8"),
]
pdf = bytearray(b"%PDF-1.4\n")
offsets = [0]
for number, obj in enumerate(objects, 1):
    offsets.append(len(pdf))
    pdf += b"%d 0 obj\n" % number + obj + b"\nendobj\n"
xref = len(pdf)
pdf += b"xref\n0 %d\n0000000000 65535 f \n" % len(offsets)
for offset in offsets[1:]:
    pdf += b"%010d 00000 n \n" % offset
pdf += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(offsets), xref)
Path(__file__).with_name("mixed.pdf").write_bytes(pdf)
