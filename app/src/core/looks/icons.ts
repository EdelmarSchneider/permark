// The icons a tag | an official name can wear: lucide components by the names the files keep.

import type { Component } from "svelte";
import Alert from "@lucide/svelte/icons/triangle-alert";
import BookOpen from "@lucide/svelte/icons/book-open";
import Bookmark from "@lucide/svelte/icons/bookmark";
import Calendar from "@lucide/svelte/icons/calendar";
import Check from "@lucide/svelte/icons/check";
import Code from "@lucide/svelte/icons/code";
import Flag from "@lucide/svelte/icons/flag";
import Folder from "@lucide/svelte/icons/folder";
import Lightbulb from "@lucide/svelte/icons/lightbulb";
import Link from "@lucide/svelte/icons/link";
import Star from "@lucide/svelte/icons/star";
import User from "@lucide/svelte/icons/user";
import type { TagIconName } from "./model";

type Icon = Component<{ size?: number }>;

export const TAG_ICONS: Record<TagIconName, Icon> = {
  star: Star,
  bookmark: Bookmark,
  flag: Flag,
  lightbulb: Lightbulb,
  check: Check,
  alert: Alert,
  "book-open": BookOpen,
  code: Code,
  folder: Folder,
  user: User,
  calendar: Calendar,
  link: Link,
};
