# Spreadsheet HTML regression fixture

`spreadsheet-with-image.xlsx` is a generated workbook containing the text
"Image export" in A1 and a 40 × 40 PNG anchored at B2. The PNG is transparent
with one red pixel. It contains no user document content.

The LibreOffice integration test converts it through the batch pipeline and
checks that its image remains decodable from the saved HTML after temporary
export files are removed.
