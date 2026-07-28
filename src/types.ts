export type FileStatus =
  | "pending"
  | "loading"
  | "compressed"
  | "best"
  | "restored"
  | "error";

export interface FileItem {
  id: string;
  path: string;
  name: string;
  originSize: number;
  outputSize: number;
  ratio: number;
  status: FileStatus;
  message: string;
  selected: boolean;
}

export interface CompressResult {
  path: string;
  name: string;
  originSize: number;
  outputSize: number;
  ratio: number;
  status: string;
  message: string;
}

export interface PreviewResult {
  mime: string;
  originBase64: string;
  compressedBase64: string;
}

export function formatSize(bytes: number): string {
  if (!bytes && bytes !== 0) return "-";
  const unit = 1024;
  if (bytes < unit) return `${bytes} B`;
  return `${(bytes / unit).toFixed(2)} KB`;
}

export function createId(): string {
  return crypto.randomUUID();
}
