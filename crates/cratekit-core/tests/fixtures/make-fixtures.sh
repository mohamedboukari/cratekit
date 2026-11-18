#!/bin/sh
# Regenerates the synthetic decoder test files in this folder with ffmpeg.
# They are pure tones made by a formula: no recorded or copyrighted audio.
#
# tone.*         0.5 s, 44.1 kHz stereo: left 440 Hz at 0.5, right 1 kHz at 0.25
# tone-alac.m4a  the stereo tone in Apple Lossless (16-bit, so the same samples as tone.wav)
# mono-48k.wav   0.5 s, 48 kHz mono, 24-bit: 1 kHz at 0.5
# rate-change.mp3  1 s at 44.1 kHz then 1 s at 48 kHz in one MP3 stream (no
#                  Xing header), mono 440 Hz: a file whose format changes
# tagged.*       0.2 s 440 Hz (mono; Ogg stereo) with title "Test Tone", artist "Cratekit",
#                album "Fixtures" (ID3v2, Vorbis comments, MP4 tags)
#
# Run from anywhere: sh crates/cratekit-core/tests/fixtures/make-fixtures.sh
set -eu
cd "$(dirname "$0")"

STEREO='aevalsrc=0.5*sin(2*PI*440*t)|0.25*sin(2*PI*1000*t):s=44100:d=0.5'
MONO='aevalsrc=0.5*sin(2*PI*1000*t):s=48000:d=0.5'
IN='-hide_banner -loglevel error -y -f lavfi -i'
OUT='-map_metadata -1 -fflags +bitexact -flags:a +bitexact'

# shellcheck disable=SC2086 # IN and OUT are lists of flags on purpose.
{
  ffmpeg $IN "$STEREO" $OUT -c:a pcm_s16le tone.wav
  ffmpeg $IN "$STEREO" $OUT -c:a pcm_s16be tone.aiff
  ffmpeg $IN "$STEREO" $OUT -c:a flac -sample_fmt s16 tone.flac
  ffmpeg $IN "$STEREO" $OUT -c:a libmp3lame -b:a 192k tone.mp3
  ffmpeg $IN "$STEREO" $OUT -c:a aac -b:a 192k tone.m4a
  ffmpeg $IN "$STEREO" $OUT -c:a vorbis -strict experimental -q:a 6 tone.ogg
  ffmpeg $IN "$STEREO" $OUT -c:a alac -sample_fmt s16p tone-alac.m4a
  ffmpeg $IN "$MONO" $OUT -c:a pcm_s24le mono-48k.wav
  for rate in 44100 48000; do
    ffmpeg $IN "aevalsrc=0.5*sin(2*PI*440*t):s=$rate:d=1" $OUT \
      -c:a libmp3lame -b:a 64k -write_xing 0 "part-$rate.mp3"
  done
  cat part-44100.mp3 part-48000.mp3 > rate-change.mp3
  rm part-44100.mp3 part-48000.mp3
  tagged() {
    ffmpeg $IN 'aevalsrc=0.5*sin(2*PI*440*t):s=44100:d=0.2' $OUT \
      -metadata 'title=Test Tone' -metadata artist=Cratekit -metadata album=Fixtures "$@"
  }
  tagged -c:a libmp3lame -b:a 64k tagged.mp3
  tagged -c:a flac -sample_fmt s16 tagged.flac
  tagged -c:a aac -b:a 64k tagged.m4a
  tagged -ac 2 -c:a vorbis -strict experimental -q:a 2 tagged.ogg
}
