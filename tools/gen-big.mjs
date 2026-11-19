import { writeFileSync } from "node:fs";

const out = process.argv[2] || "assets/big.md";
const target = Number(process.argv[3] || 1048576);
const para =
  "第 N 段：中文长文压测。阅读器的初始布局、滚动重建与内存都应被测量，而不是猜。" +
  "混排 English words and numbers 0123456789，以及中文标点：，。；：？！「」『』（）【】。" +
  "这一段会重复到文件达到目标体积。\n\n";
let text = "# 一兆字节压测\n\n";
let index = 0;
while (Buffer.byteLength(text, "utf8") < target) {
  index += 1;
  text += para.replace("N", String(index));
}
writeFileSync(out, text);
console.log(`${out} ${Buffer.byteLength(text, "utf8")} bytes ${index} paragraphs`);
