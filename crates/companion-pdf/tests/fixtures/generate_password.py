"""Generate outline.pdf and its encrypted twin (test password: companion).

Requires pypdf==6.21.3; RC4-128 avoids an optional crypto dependency.
No production password or credential is used.
"""
from pathlib import Path
from pypdf import PdfReader, PdfWriter

here = Path(__file__).parent
writer = PdfWriter()
for page in PdfReader(here / "mixed.pdf").pages:
    writer.add_page(page)
writer.add_outline_item("Text chapter", 0)
writer.add_outline_item("Scanned chapter", 1)
writer.write(here / "outline.pdf")
writer.encrypt("companion", owner_password="fixture-owner", algorithm="RC4-128")
writer.write(here / "encrypted.pdf")
