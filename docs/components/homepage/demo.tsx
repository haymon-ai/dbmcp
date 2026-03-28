import Image from 'next/image';

export function Demo() {
  return (
    <section className="w-full bg-black py-8 md:py-12">
      <div className="mx-auto max-w-[900px]">
        <Image
          src="/demo.gif"
          alt="Demo showing natural-language database queries answered instantly"
          className="w-full rounded-sm"
          width={900}
          height={500}
        />
      </div>
    </section>
  );
}
