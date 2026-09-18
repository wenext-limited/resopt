import type { Request, Reply } from "./types";
export class Engine {
  private worker: Worker | null = null;
  private next = 0;
  private pending = new Map<
    number,
    { resolve: (value: Reply) => void; reject: (error: Error) => void }
  >();
  private start() {
    if (this.worker) return;
    this.worker = new Worker(new URL("./worker.js", document.baseURI), {
      type: "module",
    });
    this.worker.onmessage = ({ data }: MessageEvent<Reply>) => {
      const request = this.pending.get(data.id);
      if (!request) return;
      this.pending.delete(data.id);
      data.ok ? request.resolve(data) : request.reject(new Error(data.error));
    };
    this.worker.onerror = (event) => {
      event.preventDefault();
      this.cancel(new Error("图片处理进程异常，请减少图片尺寸后重试。"));
    };
  }
  private call(
    data:
      | Omit<Extract<Request, { kind: "init" }>, "id">
      | Omit<Extract<Request, { kind: "optimize" }>, "id">,
    transfer: Transferable[] = [],
  ): Promise<Reply> {
    this.start();
    const id = ++this.next;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker!.postMessage({ ...data, id }, transfer);
    });
  }
  ready() {
    return this.call({ kind: "init" });
  }
  optimize(bytes: ArrayBuffer, effort: number, reductions: boolean) {
    return this.call({ kind: "optimize", bytes, effort, reductions }, [bytes]);
  }
  cancel(error = new Error("已暂停")) {
    this.worker?.terminate();
    this.worker = null;
    for (const request of this.pending.values()) request.reject(error);
    this.pending.clear();
  }
}
