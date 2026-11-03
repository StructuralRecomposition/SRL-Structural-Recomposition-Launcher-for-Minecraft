/** Open the native file picker and resolve the chosen files. */
export function pickFiles(accept: string, multiple = true): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.multiple = multiple;
    input.style.position = "fixed";
    input.style.left = "-10000px";
    document.body.appendChild(input);
    const cleanup = () => {
      input.remove();
    };
    input.addEventListener("change", () => {
      const files = Array.from(input.files ?? []);
      cleanup();
      resolve(files);
    });
    input.addEventListener("cancel", () => {
      cleanup();
      resolve([]);
    });
    input.click();
  });
}

export const MAX_UPLOAD_BYTES = 512 * 1024 * 1024;

export async function readFileBytes(file: File): Promise<Uint8Array> {
  return new Uint8Array(await file.arrayBuffer());
}

export function isExtension(path: string, extensions: string[]): boolean {
  const lower = path.toLowerCase();
  return extensions.some((ext) => lower.endsWith(ext));
}
